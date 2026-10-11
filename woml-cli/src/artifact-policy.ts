import { readFile, realpath, stat } from 'node:fs/promises';
import { dirname, isAbsolute, resolve } from 'node:path';

import type { ProcessRuntimeMode } from './process-policy';

export interface ArtifactPolicyV1 {
  readonly contract: 'woml.artifact-policy';
  readonly contractVersion: 1;
  readonly enabled: boolean;
  readonly modes: { readonly foreground: boolean; readonly background: boolean };
  readonly projectRoot: string;
  readonly readRoots: readonly string[];
  readonly materialization: {
    readonly directory: string;
    readonly cleanupOnStart: true;
    readonly maxBytesPerAttempt: number;
  };
  readonly limits: {
    readonly maxArtifactBytes: number;
    readonly maxRunBytes: number;
    readonly maxStoreBytes: number;
    readonly maxConcurrentTransfers: number;
    readonly transferTimeoutMs: number;
  };
  readonly retention: {
    readonly defaultMs: number;
    readonly minimumMs: number;
    readonly maximumMs: number;
  };
  readonly secretScanning: {
    readonly knownSecretAction: 'reject' | 'quarantine' | 'allow';
    readonly minimumSecretBytes: 8;
  };
  readonly backend: {
    readonly contract: 'woml.artifact-backend';
    readonly contractVersion: 1;
    readonly kind: 'local';
    readonly directory: string;
  };
}

export interface ArtifactRuntimeAuthorityV1 {
  readonly artifacts: {
    readonly mode: ProcessRuntimeMode;
    readonly policy: ArtifactPolicyV1;
  };
}

export class ArtifactPolicyError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(`${message} Corrective action: provide a valid, operator-owned Artifact Policy v1 document.`);
    this.name = 'ArtifactPolicyError';
    this.code = code;
  }
}

export async function loadArtifactPolicy(
  path: string,
  mode: ProcessRuntimeMode
): Promise<ArtifactRuntimeAuthorityV1> {
  const policyPath = resolve(path);
  let source: string;
  try {
    const metadata = await stat(policyPath);
    if (!metadata.isFile() || metadata.size > 1024 * 1024) invalid('The artifact policy must be a JSON file no larger than 1 MiB.');
    source = await readFile(policyPath, 'utf8');
  } catch (error) {
    if (error instanceof ArtifactPolicyError) throw error;
    throw new ArtifactPolicyError(
      'WOML_ARTIFACT_POLICY_NOT_FOUND',
      `The artifact policy could not be read: ${policyPath}`
    );
  }
  let decoded: unknown;
  try {
    decoded = JSON.parse(source);
  } catch {
    invalid('The artifact policy is not valid JSON.');
  }
  return {
    artifacts: {
      mode,
      policy: await preflightArtifactPolicy(decoded, dirname(policyPath), mode),
    },
  };
}

export function artifactGrantSummary(authority: ArtifactRuntimeAuthorityV1): string {
  return `Artifact capability (${authority.artifacts.mode}): local immutable backend enabled`;
}

export async function preflightArtifactPolicy(
  input: unknown,
  baseDirectory: string,
  mode: ProcessRuntimeMode
): Promise<ArtifactPolicyV1> {
  const policy = strictObject(input, 'policy', [
    'contract', 'contractVersion', 'enabled', 'modes', 'projectRoot', 'readRoots',
    'materialization', 'limits', 'retention', 'secretScanning', 'backend',
  ]);
  if (policy.contract !== 'woml.artifact-policy' || policy.contractVersion !== 1) invalid('The document does not match Artifact Policy v1.');
  if (policy.enabled !== true) throw new ArtifactPolicyError('WOML_ARTIFACT_SERVICE_DISABLED', 'Durable artifacts are disabled by policy.');
  const modes = strictObject(policy.modes, 'modes', ['foreground', 'background']);
  if (typeof modes.foreground !== 'boolean' || typeof modes.background !== 'boolean' || modes[mode] !== true) {
    throw new ArtifactPolicyError('WOML_ARTIFACT_SERVICE_DISABLED', `Durable artifacts are not enabled in ${mode} mode.`);
  }
  const projectRootValue = boundedString(policy.projectRoot, 'projectRoot', 4096);
  const projectRoot = await canonicalDirectory(resolve(baseDirectory, projectRootValue), 'projectRoot');
  if (!Array.isArray(policy.readRoots) || policy.readRoots.length < 1 || policy.readRoots.length > 128) invalid('readRoots must contain 1 through 128 paths.');
  const readRoots: string[] = [];
  for (const value of policy.readRoots) {
    const authored = boundedString(value, 'readRoots entry', 4096);
    const root = await canonicalDirectory(isAbsolute(authored) ? authored : resolve(projectRoot, authored), 'read root');
    if (root !== projectRoot && !root.startsWith(`${projectRoot}/`)) invalid('Every read root must remain beneath projectRoot.');
    if (!readRoots.includes(root)) readRoots.push(root);
  }
  const materialization = strictObject(policy.materialization, 'materialization', ['directory', 'cleanupOnStart', 'maxBytesPerAttempt']);
  if (materialization.cleanupOnStart !== true) invalid('materialization.cleanupOnStart must be true in v1.');
  const limits = strictObject(policy.limits, 'limits', ['maxArtifactBytes', 'maxRunBytes', 'maxStoreBytes', 'maxConcurrentTransfers', 'transferTimeoutMs']);
  const retention = strictObject(policy.retention, 'retention', ['defaultMs', 'minimumMs', 'maximumMs']);
  const secretScanning = strictObject(policy.secretScanning, 'secretScanning', ['knownSecretAction', 'minimumSecretBytes']);
  const backend = strictObject(policy.backend, 'backend', ['contract', 'contractVersion', 'kind', 'directory']);
  if (backend.contract !== 'woml.artifact-backend' || backend.contractVersion !== 1 || backend.kind !== 'local') invalid('DA3 supports Artifact Backend v1 kind local only.');
  const normalized: ArtifactPolicyV1 = {
    contract: 'woml.artifact-policy', contractVersion: 1, enabled: true,
    modes: { foreground: modes.foreground as boolean, background: modes.background as boolean },
    projectRoot,
    readRoots,
    materialization: {
      directory: resolvePath(projectRoot, boundedString(materialization.directory, 'materialization.directory', 4096)),
      cleanupOnStart: true,
      maxBytesPerAttempt: integer(materialization.maxBytesPerAttempt, 1, 4_398_046_511_104, 'maxBytesPerAttempt'),
    },
    limits: {
      maxArtifactBytes: integer(limits.maxArtifactBytes, 1, 1_099_511_627_776, 'maxArtifactBytes'),
      maxRunBytes: integer(limits.maxRunBytes, 1, 4_398_046_511_104, 'maxRunBytes'),
      maxStoreBytes: integer(limits.maxStoreBytes, 1, 1_125_899_906_842_624, 'maxStoreBytes'),
      maxConcurrentTransfers: integer(limits.maxConcurrentTransfers, 1, 64, 'maxConcurrentTransfers'),
      transferTimeoutMs: integer(limits.transferTimeoutMs, 1000, 86_400_000, 'transferTimeoutMs'),
    },
    retention: {
      defaultMs: integer(retention.defaultMs, 3_600_000, 31_536_000_000, 'retention.defaultMs'),
      minimumMs: integer(retention.minimumMs, 3_600_000, 31_536_000_000, 'retention.minimumMs'),
      maximumMs: integer(retention.maximumMs, 3_600_000, 31_536_000_000, 'retention.maximumMs'),
    },
    secretScanning: {
      knownSecretAction: enumValue(secretScanning.knownSecretAction, ['reject', 'quarantine', 'allow'], 'knownSecretAction'),
      minimumSecretBytes: exactInteger(secretScanning.minimumSecretBytes, 8, 'minimumSecretBytes'),
    },
    backend: {
      contract: 'woml.artifact-backend', contractVersion: 1, kind: 'local',
      directory: resolvePath(projectRoot, boundedString(backend.directory, 'backend.directory', 4096)),
    },
  };
  if (normalized.limits.maxRunBytes < normalized.limits.maxArtifactBytes || normalized.limits.maxStoreBytes < normalized.limits.maxArtifactBytes || normalized.retention.minimumMs > normalized.retention.defaultMs || normalized.retention.defaultMs > normalized.retention.maximumMs) invalid('Artifact limits and retention bounds are inconsistent.');
  return normalized;
}

function strictObject(value: unknown, label: string, fields: readonly string[]): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) invalid(`${label} must be an object.`);
  const object = value as Record<string, unknown>;
  if (Object.keys(object).some(key => !fields.includes(key)) || fields.some(key => !(key in object))) invalid(`${label} has missing or unknown fields.`);
  return object;
}
function boundedString(value: unknown, label: string, max: number): string {
  if (typeof value !== 'string' || value.length < 1 || Buffer.byteLength(value) > max) invalid(`${label} is invalid.`);
  return value;
}
function integer(value: unknown, min: number, max: number, label: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < min || (value as number) > max) invalid(`${label} is outside the v1 bounds.`);
  return value as number;
}
function exactInteger<T extends number>(value: unknown, expected: T, label: string): T {
  if (value !== expected) invalid(`${label} must be ${expected}.`);
  return expected;
}
function enumValue<T extends string>(value: unknown, choices: readonly T[], label: string): T {
  if (typeof value !== 'string' || !choices.includes(value as T)) invalid(`${label} is invalid.`);
  return value as T;
}
function resolvePath(root: string, value: string): string {
  return isAbsolute(value) ? resolve(value) : resolve(root, value);
}
async function canonicalDirectory(path: string, label: string): Promise<string> {
  try {
    const canonical = await realpath(path);
    if (!(await stat(canonical)).isDirectory()) invalid(`${label} must be an existing directory.`);
    return canonical;
  } catch (error) {
    if (error instanceof ArtifactPolicyError) throw error;
    invalid(`${label} must be an existing directory.`);
  }
}
function invalid(message: string): never {
  throw new ArtifactPolicyError('WOML_ARTIFACT_POLICY_INVALID', message);
}
