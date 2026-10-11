import { constants } from 'node:fs';
import { access, readFile, realpath, stat } from 'node:fs/promises';
import { dirname, isAbsolute, relative, resolve } from 'node:path';

export type ProcessRuntimeMode = 'foreground' | 'background';

export interface ProcessPolicyV1 {
  readonly contract: 'woml.process-policy';
  readonly contractVersion: 1;
  readonly enabled: boolean;
  readonly modes: {
    readonly foreground: boolean;
    readonly background: boolean;
  };
  readonly projectRoot: string;
  readonly executables: readonly {
    readonly alias: string;
    readonly path: string;
  }[];
  readonly workingDirectories: readonly string[];
  readonly environment: {
    readonly inherit: readonly string[];
    readonly allow: readonly string[];
  };
  readonly limits: {
    readonly maxTimeoutMs: number;
    readonly maxOutputBytes: number;
    readonly maxConcurrent: number;
  };
}

export interface ProcessRuntimeAuthorityV1 {
  readonly process: {
    readonly mode: ProcessRuntimeMode;
    readonly policy: ProcessPolicyV1;
  };
}

export class ProcessPolicyError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'ProcessPolicyError';
    this.code = code;
  }
}

const aliasPattern = /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/;
const environmentPattern = /^[A-Za-z_][A-Za-z0-9_]{0,127}$/;

export async function loadProcessPolicy(
  path: string,
  mode: ProcessRuntimeMode
): Promise<ProcessRuntimeAuthorityV1> {
  const policyPath = resolve(path);
  let source: string;
  try {
    const metadata = await stat(policyPath);
    if (!metadata.isFile() || metadata.size > 1024 * 1024) {
      throw new ProcessPolicyError(
        'WOML_PROCESS_POLICY_INVALID',
        'The process policy must be a JSON file no larger than 1 MiB.'
      );
    }
    source = await readFile(policyPath, 'utf8');
  } catch (error) {
    if (error instanceof ProcessPolicyError) throw error;
    throw new ProcessPolicyError(
      'WOML_PROCESS_POLICY_NOT_FOUND',
      `The process policy could not be read: ${policyPath}`
    );
  }
  let decoded: unknown;
  try {
    decoded = JSON.parse(source);
  } catch {
    throw new ProcessPolicyError(
      'WOML_PROCESS_POLICY_INVALID',
      'The process policy is not valid JSON.'
    );
  }
  return {
    process: {
      mode,
      policy: await preflightProcessPolicy(decoded, dirname(policyPath), mode),
    },
  };
}

export async function createForegroundProcessGrant(
  aliases: string,
  projectRoot = process.cwd()
): Promise<ProcessRuntimeAuthorityV1> {
  const requested = aliases
    .split(',')
    .map(value => value.trim())
    .filter(value => value.length > 0);
  if (requested.length === 0 || new Set(requested).size !== requested.length) {
    throw new ProcessPolicyError(
      'WOML_PROCESS_GRANT_INVALID',
      '--allow-process requires a unique comma-separated list such as bun,git.'
    );
  }
  const executables: { alias: string; path: string }[] = [];
  for (const alias of requested) {
    if (!aliasPattern.test(alias)) {
      throw new ProcessPolicyError(
        'WOML_PROCESS_GRANT_INVALID',
        `Invalid process executable alias: ${alias}`
      );
    }
    const path = Bun.which(alias);
    if (path === null) {
      throw new ProcessPolicyError(
        'WOML_PROCESS_EXECUTABLE_NOT_FOUND',
        `Executable "${alias}" was not found in the operator PATH.`
      );
    }
    executables.push({ alias, path });
  }
  return {
    process: {
      mode: 'foreground',
      policy: await preflightProcessPolicy(
        {
          contract: 'woml.process-policy',
          contractVersion: 1,
          enabled: true,
          modes: { foreground: true, background: false },
          projectRoot,
          executables,
          workingDirectories: ['.'],
          environment: { inherit: [], allow: [] },
          limits: {
            maxTimeoutMs: 86_400_000,
            maxOutputBytes: 262_144,
            maxConcurrent: 4,
          },
        },
        process.cwd(),
        'foreground'
      ),
    },
  };
}

export async function preflightProcessPolicy(
  value: unknown,
  baseDirectory: string,
  mode: ProcessRuntimeMode
): Promise<ProcessPolicyV1> {
  const policy = strictObject(value, 'process policy', [
    'contract',
    'contractVersion',
    'enabled',
    'modes',
    'projectRoot',
    'executables',
    'workingDirectories',
    'environment',
    'limits',
  ]);
  if (
    policy.contract !== 'woml.process-policy' ||
    policy.contractVersion !== 1
  ) {
    invalid('The policy must use contract "woml.process-policy" version 1.');
  }
  if (typeof policy.enabled !== 'boolean' || !policy.enabled) {
    throw new ProcessPolicyError(
      'WOML_PROCESS_DISABLED',
      'Process execution is disabled by runtime policy.'
    );
  }
  const modes = strictObject(policy.modes, 'modes', [
    'foreground',
    'background',
  ]);
  if (
    typeof modes.foreground !== 'boolean' ||
    typeof modes.background !== 'boolean'
  ) {
    invalid(
      'Process policy modes must contain foreground and background booleans.'
    );
  }
  if (modes[mode] !== true) {
    throw new ProcessPolicyError(
      'WOML_PROCESS_MODE_DENIED',
      `The process policy does not permit ${mode} execution.`
    );
  }
  if (
    typeof policy.projectRoot !== 'string' ||
    policy.projectRoot.length === 0 ||
    policy.projectRoot.length > 4096
  ) {
    invalid('Process policy projectRoot must be a non-empty path.');
  }
  const projectRoot = await canonicalDirectory(
    isAbsolute(policy.projectRoot)
      ? policy.projectRoot
      : resolve(baseDirectory, policy.projectRoot),
    'WOML_PROCESS_PROJECT_ROOT_INVALID',
    'project root'
  );
  if (!Array.isArray(policy.executables) || policy.executables.length > 128) {
    invalid(
      'Process policy executables must be an array with at most 128 entries.'
    );
  }
  const aliases = new Set<string>();
  const executables: { alias: string; path: string }[] = [];
  for (const raw of policy.executables) {
    const entry = strictObject(raw, 'executable', ['alias', 'path']);
    if (
      typeof entry.alias !== 'string' ||
      !aliasPattern.test(entry.alias) ||
      aliases.has(entry.alias) ||
      typeof entry.path !== 'string' ||
      entry.path.length === 0 ||
      entry.path.length > 4096
    ) {
      invalid(
        'Every executable requires a unique valid alias and a non-empty path.'
      );
    }
    aliases.add(entry.alias);
    const candidate = isAbsolute(entry.path)
      ? entry.path
      : resolve(baseDirectory, entry.path);
    let canonical: string;
    try {
      canonical = await realpath(candidate);
      const metadata = await stat(canonical);
      if (!metadata.isFile()) throw new Error('not a file');
      if (process.platform !== 'win32') await access(canonical, constants.X_OK);
    } catch {
      throw new ProcessPolicyError(
        'WOML_PROCESS_EXECUTABLE_NOT_FOUND',
        `Executable alias "${entry.alias}" does not resolve to an executable file.`
      );
    }
    executables.push({ alias: entry.alias, path: canonical });
  }
  if (
    !Array.isArray(policy.workingDirectories) ||
    policy.workingDirectories.length < 1 ||
    policy.workingDirectories.length > 128
  ) {
    invalid('Process policy requires 1 to 128 working-directory roots.');
  }
  const workingDirectories: string[] = [];
  const authoredWorkingDirectories = new Set<string>();
  for (const raw of policy.workingDirectories) {
    if (
      typeof raw !== 'string' ||
      raw.length === 0 ||
      raw.length > 4096 ||
      authoredWorkingDirectories.has(raw)
    ) {
      invalid('Every working-directory root must be a non-empty path.');
    }
    authoredWorkingDirectories.add(raw);
    const candidate = isAbsolute(raw) ? raw : resolve(projectRoot, raw);
    const canonical = await canonicalDirectory(
      candidate,
      'WOML_PROCESS_CWD_NOT_FOUND',
      'working-directory root'
    );
    if (!inside(projectRoot, canonical)) {
      throw new ProcessPolicyError(
        'WOML_PROCESS_CWD_DENIED',
        'A process working-directory root escapes the project root.'
      );
    }
    if (!workingDirectories.includes(canonical))
      workingDirectories.push(canonical);
  }
  const environment = strictObject(policy.environment, 'environment', [
    'inherit',
    'allow',
  ]);
  const inherit = stringSet(
    environment.inherit,
    'environment.inherit',
    environmentPattern,
    128
  );
  const allow = stringSet(
    environment.allow,
    'environment.allow',
    environmentPattern,
    128
  );
  const limits = strictObject(policy.limits, 'limits', [
    'maxTimeoutMs',
    'maxOutputBytes',
    'maxConcurrent',
  ]);
  const maxTimeoutMs = boundedInteger(
    limits.maxTimeoutMs,
    1,
    86_400_000,
    'maxTimeoutMs'
  );
  const maxOutputBytes = boundedInteger(
    limits.maxOutputBytes,
    1,
    262_144,
    'maxOutputBytes'
  );
  const maxConcurrent = boundedInteger(
    limits.maxConcurrent,
    1,
    64,
    'maxConcurrent'
  );
  return {
    contract: 'woml.process-policy',
    contractVersion: 1,
    enabled: true,
    modes: { foreground: modes.foreground, background: modes.background },
    projectRoot,
    executables,
    workingDirectories,
    environment: { inherit, allow },
    limits: { maxTimeoutMs, maxOutputBytes, maxConcurrent },
  };
}

export function processGrantSummary(
  authority: ProcessRuntimeAuthorityV1
): string {
  const aliases = authority.process.policy.executables
    .map(entry => `${entry.alias} → ${entry.path}`)
    .join(', ');
  return `Process capability (${authority.process.mode}): ${aliases || 'no executables granted'}`;
}

function strictObject(
  value: unknown,
  label: string,
  allowed: readonly string[]
): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    invalid(`The ${label} must be an object.`);
  }
  const object = value as Record<string, unknown>;
  const unknown = Object.keys(object).find(key => !allowed.includes(key));
  if (unknown !== undefined || allowed.some(key => !(key in object))) {
    invalid(`The ${label} has missing or unknown fields.`);
  }
  return object;
}

function stringSet(
  value: unknown,
  label: string,
  pattern: RegExp,
  maximum: number
): readonly string[] {
  if (!Array.isArray(value) || value.length > maximum)
    invalid(`${label} is invalid.`);
  const values = value as unknown[];
  if (
    values.some(item => typeof item !== 'string' || !pattern.test(item)) ||
    new Set(values).size !== values.length
  ) {
    invalid(`${label} must contain unique valid names.`);
  }
  return values as string[];
}

function boundedInteger(
  value: unknown,
  minimum: number,
  maximum: number,
  label: string
): number {
  if (
    !Number.isSafeInteger(value) ||
    (value as number) < minimum ||
    (value as number) > maximum
  ) {
    invalid(`${label} must be an integer from ${minimum} through ${maximum}.`);
  }
  return value as number;
}

async function canonicalDirectory(
  path: string,
  code: string,
  label: string
): Promise<string> {
  try {
    const canonical = await realpath(path);
    if (!(await stat(canonical)).isDirectory())
      throw new Error('not a directory');
    return canonical;
  } catch {
    throw new ProcessPolicyError(
      code,
      `The process ${label} does not exist or is not a directory.`
    );
  }
}

function inside(root: string, candidate: string): boolean {
  const value = relative(root, candidate);
  return value === '' || (!value.startsWith('..') && !isAbsolute(value));
}

function invalid(message: string): never {
  throw new ProcessPolicyError('WOML_PROCESS_POLICY_INVALID', message);
}
