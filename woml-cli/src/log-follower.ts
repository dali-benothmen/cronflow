import { stat } from 'node:fs/promises';
import { resolve } from 'node:path';

import {
  RunManagementError,
  hasWorkflowDefinitionWithRust,
  inspectRunPresentationWithRust,
  listRunPresentationsWithRust,
  readOperationalOutputPageWithRust,
} from './rust-executor';
import {
  readRuntimeDescriptor,
  runtimeDescriptorPath,
  RuntimeControlError,
  type RuntimeDescriptorV1,
} from './runtime-control';
import { resolveRuntimeConfiguration } from './runtime-config';
import { consumeOperationsStream } from './operations-stream';
import {
  decodeRunPresentationListV1,
  decodeRunPresentationV1,
  renderPresentationWarning,
  renderRunPresentation,
  sanitizePresentationDiagnostic,
  type ColorMode,
  type PresentationRenderOptions,
  type RunPresentationListV1,
  type RunPresentationV1,
} from './terminal-presentation';
import {
  decodeOperationalRecordV1,
  operationalSources,
  renderOperationalRecord,
  type OperationalPageV1,
  type OperationalRecordV1,
  type OperationalSourceV1,
  type OperationalStreamEventV1,
} from './operational-output';

export const logFollowUsage =
  'Usage: woml logs <run-id|workflow-id> [--follow|--no-follow] [--tail <count>|--after <cursor>] [--source <stdout,stderr,script,runtime>] [--state <path>] [--config <path>] [--json] [--color=auto|always|never]\n       woml <run-id|workflow-id> --logs [same options]';

export class LogFollowError extends Error {
  readonly code: string;

  constructor(code: string, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'LogFollowError';
    this.code = code;
  }
}

export interface LogFollowArguments {
  readonly subject: string;
  readonly subjectKind: 'run' | 'workflow';
  readonly statePath: string;
  readonly json: boolean;
  readonly color: ColorMode;
  readonly follow?: boolean;
  readonly tail?: number;
  readonly after?: number;
  readonly sources?: ReadonlySet<OperationalSourceV1>;
  readonly operational?: boolean;
}

interface ParsedLogFollowArguments extends LogFollowArguments {
  readonly stateExplicit: boolean;
  readonly configPath?: string;
}

export interface LogFollowIo {
  readonly stdout: (text: string) => void;
  readonly stderr: (text: string) => void;
  readonly isTTY?: boolean;
  readonly columns?: number;
}

type LogFetch = (input: string, init?: RequestInit) => Promise<Response>;

interface RuntimeSnapshot {
  readonly runtimeInstanceId: string;
  readonly sequence: number;
  readonly workflows: readonly string[];
  readonly runs: readonly { readonly runId: string; readonly workflowId: string }[];
}

export interface LogFollowerDependencies {
  readonly nativeCorePath?: string;
  readonly fetch?: LogFetch;
  readonly readDescriptor?: (path: string) => Promise<RuntimeDescriptorV1>;
  readonly waitForDetach?: () => Promise<void>;
  readonly reconnectWindowMs?: number;
  readonly readRun?: (statePath: string, runId: string) => RunPresentationV1;
  readonly readWorkflow?: (
    statePath: string,
    workflowId: string,
    limit: number
  ) => RunPresentationListV1;
  readonly hasWorkflow?: (statePath: string, workflowId: string) => boolean;
  readonly readOutputPage?: (
    statePath: string,
    runId: string | undefined,
    after: number,
    limit: number
  ) => OperationalPageV1;
}

const runIdPattern = /^run_[A-Za-z0-9_-]{1,252}$/;
const workflowIdPattern = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

function invalidArguments(): never {
  throw new LogFollowError('WOML_CLI_ARGUMENTS_INVALID', logFollowUsage);
}

export function parseLogFollowArguments(
  args: readonly string[]
): ParsedLogFollowArguments {
  const commandStyle = args[0] === 'logs';
  const subject = args[commandStyle ? 1 : 0];
  if (subject === undefined) invalidArguments();
  const subjectKind = runIdPattern.test(subject)
    ? 'run'
    : workflowIdPattern.test(subject) && subject.length <= 256
      ? 'workflow'
      : undefined;
  if (subjectKind === undefined) {
    throw new LogFollowError(
      'WOML_LOG_SUBJECT_INVALID',
      'The log subject must be a WOML run ID or lowercase kebab-case workflow ID.'
    );
  }

  let statePath = resolve('.woml/state.sqlite');
  let stateExplicit = false;
  let configPath: string | undefined;
  let json = false;
  let color: ColorMode = 'auto';
  let logs = commandStyle;
  let follow = true;
  let tail: number | undefined;
  let after: number | undefined;
  let sources: ReadonlySet<OperationalSourceV1> = new Set(operationalSources);
  const seen = new Set<string>();
  for (let index = commandStyle ? 2 : 1; index < args.length; index += 1) {
    const raw = args[index]!;
    const assignment = raw.startsWith('--color=')
      ? raw.slice('--color='.length)
      : undefined;
    const option = assignment === undefined ? raw : '--color';
    if (seen.has(option)) invalidArguments();
    seen.add(option);
    if (option === '--logs') {
      logs = true;
      continue;
    }
    if (option === '--json') {
      json = true;
      continue;
    }
    if (option === '--follow' || option === '--no-follow') {
      if (seen.has(option === '--follow' ? '--no-follow' : '--follow')) {
        invalidArguments();
      }
      follow = option === '--follow';
      continue;
    }
    if (
      !['--state', '--config', '--color', '--tail', '--after', '--source'].includes(
        option
      )
    ) invalidArguments();
    const value = assignment ?? args[++index];
    if (value === undefined || value.length === 0 || value.startsWith('--')) {
      invalidArguments();
    }
    if (option === '--state') {
      statePath = resolve(value);
      stateExplicit = true;
    } else if (option === '--config') {
      configPath = value;
    } else {
      if (option === '--color') {
        if (!['auto', 'always', 'never'].includes(value)) invalidArguments();
        color = value as ColorMode;
      } else if (option === '--tail') {
        tail = Number(value);
        if (!Number.isSafeInteger(tail) || tail < 1 || tail > 1_000) {
          invalidArguments();
        }
      } else if (option === '--after') {
        after = Number(value);
        if (!Number.isSafeInteger(after) || after < 0) invalidArguments();
      } else {
        const values = value.split(',').filter(Boolean);
        if (
          values.length === 0 ||
          values.some(
            source => !operationalSources.includes(source as OperationalSourceV1)
          )
        ) invalidArguments();
        sources = new Set(values as OperationalSourceV1[]);
      }
    }
  }
  if (!logs) invalidArguments();
  if (tail !== undefined && after !== undefined) invalidArguments();
  return {
    subject,
    subjectKind,
    statePath,
    stateExplicit,
    ...(configPath === undefined ? {} : { configPath }),
    json,
    color,
    follow,
    ...(tail === undefined ? {} : { tail }),
    ...(after === undefined ? {} : { after }),
    sources,
    operational: true,
  };
}

export async function resolveLogFollowArguments(
  args: readonly string[]
): Promise<LogFollowArguments> {
  const parsed = parseLogFollowArguments(args);
  const configuration = await resolveRuntimeConfiguration(parsed.configPath, {
    ...(parsed.stateExplicit ? { statePath: parsed.statePath } : {}),
  });
  return {
    subject: parsed.subject,
    subjectKind: parsed.subjectKind,
    statePath: configuration.statePath,
    json: parsed.json,
    color: parsed.color,
    follow: parsed.follow,
    ...(parsed.tail === undefined ? {} : { tail: parsed.tail }),
    ...(parsed.after === undefined ? {} : { after: parsed.after }),
    sources: parsed.sources,
    operational: parsed.operational,
  };
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

async function runtimeSnapshot(
  descriptor: RuntimeDescriptorV1,
  fetcher: LogFetch,
  signal?: AbortSignal
): Promise<RuntimeSnapshot> {
  const response = await fetcher(`${descriptor.adminUrl}/v1/snapshot`, {
    headers: { authorization: `Bearer ${descriptor.capability}` },
    signal,
  });
  if (!response.ok) {
    throw new RuntimeControlError(
      response.status === 401
        ? 'WOML_ADMIN_UNAUTHORIZED'
        : 'WOML_LOG_RUNTIME_UNAVAILABLE',
      `The active runtime snapshot returned HTTP ${response.status}.`
    );
  }
  const value: unknown = await response.json();
  if (
    !record(value) ||
    value.profile !== 'woml.runtime-operations-snapshot/v1' ||
    typeof value.runtimeInstanceId !== 'string' ||
    !Number.isSafeInteger(value.sequence) ||
    !Array.isArray(value.workflows) ||
    !Array.isArray(value.runs)
  ) {
    throw new LogFollowError(
      'WOML_LOG_RUNTIME_UNAVAILABLE',
      'The active runtime returned an invalid operations snapshot.'
    );
  }
  const workflows = value.workflows.map(item => {
    if (!record(item) || typeof item.workflowId !== 'string') {
      throw new LogFollowError(
        'WOML_LOG_RUNTIME_UNAVAILABLE',
        'The active runtime returned invalid workflow identity data.'
      );
    }
    return item.workflowId;
  });
  const runs = value.runs.map(item => {
    if (
      !record(item) ||
      typeof item.runId !== 'string' ||
      typeof item.workflowId !== 'string'
    ) {
      throw new LogFollowError(
        'WOML_LOG_RUNTIME_UNAVAILABLE',
        'The active runtime returned invalid run identity data.'
      );
    }
    return { runId: item.runId, workflowId: item.workflowId };
  });
  return {
    runtimeInstanceId: value.runtimeInstanceId,
    sequence: Number(value.sequence),
    workflows,
    runs,
  };
}

async function presentationResponse(
  descriptor: RuntimeDescriptorV1,
  path: string,
  fetcher: LogFetch,
  signal: AbortSignal
): Promise<unknown> {
  const response = await fetcher(`${descriptor.adminUrl}${path}`, {
    headers: { authorization: `Bearer ${descriptor.capability}` },
    signal,
  });
  if (!response.ok) {
    let code: string | undefined;
    try {
      const body: unknown = await response.json();
      if (
        record(body) &&
        record(body.error) &&
        typeof body.error.code === 'string'
      ) {
        code = body.error.code;
      }
    } catch {
      // The status remains enough to reject an invalid admin response.
    }
    throw new LogFollowError(
      code === 'WOML_RUN_NOT_FOUND'
        ? 'WOML_LOG_RUN_NOT_FOUND'
        : code ?? 'WOML_LOG_RUNTIME_UNAVAILABLE',
      `The active runtime presentation returned HTTP ${response.status}.`
    );
  }
  return await response.json();
}

function terminal(status: RunPresentationV1['status']): boolean {
  return ['succeeded', 'failed', 'cancelled', 'timed_out'].includes(status);
}

function inactiveDescriptor(error: unknown): boolean {
  return error instanceof Error && 'code' in error && error.code === 'ENOENT';
}

function stateAccessDenied(error: unknown): boolean {
  return error instanceof Error && 'code' in error &&
    (error.code === 'EACCES' || error.code === 'EPERM');
}

function stateAccessError(error: unknown): LogFollowError {
  return new LogFollowError(
    'WOML_LOG_STATE_UNAVAILABLE',
    'The durable state file cannot be read with the current user permissions.',
    { cause: error }
  );
}

async function existingState(path: string): Promise<boolean> {
  try {
    return (await stat(path)).isFile();
  } catch (error) {
    if (inactiveDescriptor(error)) return false;
    if (stateAccessDenied(error)) throw stateAccessError(error);
    throw error;
  }
}

function operationalEvent(value: unknown): OperationalStreamEventV1 {
  if (
    !record(value) ||
    value.profile !== 'woml.operational-stream/v1' ||
    !Number.isSafeInteger(value.cursor) ||
    Number(value.cursor) < 0
  ) {
    throw new LogFollowError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The runtime returned an invalid operational-output event.'
    );
  }
  if (value.event === 'record') {
    return {
      profile: 'woml.operational-stream/v1',
      event: 'record',
      cursor: Number(value.cursor),
      record: decodeOperationalRecordV1(value.record),
    };
  }
  if (
    value.event === 'gap' &&
    record(value.gap) &&
    Number.isSafeInteger(value.gap.requestedAfter) &&
    Number.isSafeInteger(value.gap.prunedThrough)
  ) {
    return value as unknown as OperationalStreamEventV1;
  }
  if (value.event === 'heartbeat' || value.event === 'closed') {
    return value as unknown as OperationalStreamEventV1;
  }
  throw new LogFollowError(
    'WOML_OUTPUT_PROTOCOL_INVALID',
    'The runtime returned an unknown operational-output event.'
  );
}

async function consumeOperationalStream(
  response: Response,
  onEvent: (event: OperationalStreamEventV1) => void,
  signal: AbortSignal
): Promise<void> {
  if (!response.ok || response.body === null) {
    let code = response.status === 401
      ? 'WOML_ADMIN_UNAUTHORIZED'
      : 'WOML_OUTPUT_STREAM_UNAVAILABLE';
    try {
      const value: unknown = await response.json();
      if (record(value) && record(value.error) && typeof value.error.code === 'string') {
        code = value.error.code;
      }
    } catch {
      // The status and stable fallback code are sufficient.
    }
    throw new LogFollowError(
      code,
      `The operational-output stream returned HTTP ${response.status}.`
    );
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  const abort = (): void => { void reader.cancel(); };
  signal.addEventListener('abort', abort, { once: true });
  try {
    while (!signal.aborted) {
      const { done, value } = await reader.read();
      buffer += decoder.decode(value, { stream: !done });
      let boundary: number;
      while ((boundary = buffer.search(/\r?\n\r?\n/u)) >= 0) {
        const frame = buffer.slice(0, boundary);
        const delimiter = buffer.slice(boundary).startsWith('\r\n\r\n') ? 4 : 2;
        buffer = buffer.slice(boundary + delimiter);
        const data = frame
          .split(/\r?\n/u)
          .filter(line => line.startsWith('data:'))
          .map(line => line.slice(5).trimStart())
          .join('\n');
        if (data.length > 0) onEvent(operationalEvent(JSON.parse(data)));
      }
      if (done) break;
    }
  } finally {
    signal.removeEventListener('abort', abort);
    reader.releaseLock();
  }
}

async function followOperationalLogs(options: {
  readonly args: LogFollowArguments;
  readonly io: LogFollowIo;
  readonly dependencies: LogFollowerDependencies;
}): Promise<number> {
  const { args, io, dependencies } = options;
  const fetcher = dependencies.fetch ?? globalThis.fetch;
  const readDescriptor = dependencies.readDescriptor ?? readRuntimeDescriptor;
  const readRun =
    dependencies.readRun ??
    ((statePath: string, runId: string) =>
      inspectRunPresentationWithRust(statePath, runId, {
        nativeCorePath: dependencies.nativeCorePath,
      }));
  const readWorkflow =
    dependencies.readWorkflow ??
    ((statePath: string, workflowId: string, limit: number) =>
      listRunPresentationsWithRust(statePath, workflowId, limit, {
        nativeCorePath: dependencies.nativeCorePath,
      }));
  const hasWorkflow =
    dependencies.hasWorkflow ??
    ((statePath: string, workflowId: string) =>
      hasWorkflowDefinitionWithRust(statePath, workflowId, {
        nativeCorePath: dependencies.nativeCorePath,
      }));
  const readOutput =
    dependencies.readOutputPage ??
    ((statePath: string, runId: string | undefined, after: number, limit: number) =>
      readOperationalOutputPageWithRust(
        statePath,
        { ...(runId === undefined ? {} : { runId }), after, limit },
        { nativeCorePath: dependencies.nativeCorePath }
      ));
  const sources = args.sources ?? new Set(operationalSources);
  const descriptorFile = runtimeDescriptorPath(args.statePath);
  const stateAvailable =
    dependencies.readOutputPage !== undefined ||
    dependencies.readRun !== undefined ||
    dependencies.readWorkflow !== undefined ||
    (await existingState(args.statePath));
  let retainedRun: RunPresentationV1 | undefined;
  let retainedWorkflow: RunPresentationListV1 | undefined;
  let retainedWorkflowDefinition = false;
  if (stateAvailable) {
    try {
      if (args.subjectKind === 'run') retainedRun = readRun(args.statePath, args.subject);
      else {
        retainedWorkflow = readWorkflow(args.statePath, args.subject, 10);
        retainedWorkflowDefinition = hasWorkflow(args.statePath, args.subject);
      }
    } catch (error) {
      if (
        !(error instanceof RunManagementError && error.code === 'WOML_RUN_NOT_FOUND')
      ) throw error;
    }
  }

  let cursor = args.after ?? 0;
  let outputCount = 0;
  let streamClosed = false;
  let interactiveProgress = false;
  const tail: OperationalRecordV1[] = [];
  let collectingTail = args.tail !== undefined;
  const writeRecord = (item: OperationalRecordV1): void => {
    if (args.json) {
      io.stdout(`${JSON.stringify(item)}\n`);
      return;
    }
    const rendered = renderOperationalRecord(item, {
      color: args.color,
      includeRunLabel: args.subjectKind === 'workflow',
      unicode: process.env.TERM !== 'dumb',
      isTTY: io.isTTY === true,
      width: io.columns,
    });
    if (io.isTTY === true && item.kind === 'progress' && item.status === 'active') {
      io.stdout(`\r\u001b[2K${rendered}`);
      interactiveProgress = true;
      return;
    }
    if (interactiveProgress) {
      io.stdout('\r\u001b[2K');
      interactiveProgress = false;
    }
    io.stdout(`${rendered}\n`);
  };
  const clearInteractiveProgress = (): void => {
    if (!interactiveProgress) return;
    io.stdout('\r\u001b[2K');
    interactiveProgress = false;
  };
  const output = (item: OperationalRecordV1): void => {
    if (
      (args.subjectKind === 'run' ? item.runId : item.workflowId) !== args.subject ||
      ((item.kind === 'output' || item.kind === 'truncated') &&
        !sources.has(item.source))
    ) return;
    if (collectingTail && args.tail !== undefined) {
      tail.push(item);
      if (tail.length > args.tail) tail.shift();
      return;
    }
    outputCount += 1;
    writeRecord(item);
  };
  const flushTail = (): void => {
    for (const item of tail) {
      outputCount += 1;
      writeRecord(item);
    }
    tail.length = 0;
    collectingTail = false;
  };
  const replay = (): void => {
    if (!stateAvailable) return;
    let pages = 0;
    while (pages < 100_000) {
      const page = readOutput(
        args.statePath,
        args.subjectKind === 'run' ? args.subject : undefined,
        cursor,
        1_000
      );
      if (page.gap !== undefined && !args.json) {
        io.stderr(
          `Warning [WOML_OUTPUT_REPLAY_GAP]: Output before sequence ${page.gap.prunedThrough} is no longer retained.\n`
        );
      }
      for (const item of page.records) {
        output(item);
        if (
          args.subjectKind === 'run' &&
          item.runId === args.subject &&
          item.kind === 'closed'
        ) streamClosed = true;
      }
      const previous = cursor;
      cursor = Math.max(cursor, page.nextAfter);
      pages += 1;
      if (!page.hasMore || cursor === previous) break;
    }
    if (collectingTail) flushTail();
  };
  replay();
  if (retainedRun !== undefined && terminal(retainedRun.status)) {
    streamClosed = true;
  }

  let descriptor: RuntimeDescriptorV1 | undefined;
  let snapshot: RuntimeSnapshot | undefined;
  try {
    descriptor = await readDescriptor(descriptorFile);
    snapshot = await runtimeSnapshot(descriptor, fetcher);
  } catch (error) {
    if (
      !inactiveDescriptor(error) &&
      error instanceof RuntimeControlError &&
      error.code === 'WOML_RUNTIME_DESCRIPTOR_UNSAFE'
    ) throw error;
  }
  const activeSubject = args.subjectKind === 'workflow'
    ? snapshot?.workflows.includes(args.subject) === true
    : snapshot?.runs.some(item => item.runId === args.subject) === true;
  const hasRetained =
    retainedRun !== undefined ||
    retainedWorkflowDefinition ||
    (retainedWorkflow?.runs.length ?? 0) > 0 ||
    outputCount > 0;
  if (!hasRetained && !activeSubject) {
    throw new LogFollowError(
      args.subjectKind === 'run'
        ? 'WOML_LOG_RUN_NOT_FOUND'
        : 'WOML_LOG_WORKFLOW_NOT_FOUND',
      `${args.subjectKind === 'run' ? 'Run' : 'Workflow'} ${args.subject} has no retained history.`
    );
  }
  const finalPresentation = (): void => {
    clearInteractiveProgress();
    if (args.json || args.subjectKind !== 'run') return;
    try {
      const run = readRun(args.statePath, args.subject);
      if (terminal(run.status)) {
        io.stdout(
          `\n${renderRunPresentation(run, {
            format: io.isTTY === true ? 'tty' : 'plain',
            color: args.color,
            isTTY: io.isTTY === true,
            width: io.columns,
            environment: process.env,
            fullResultCommand: runId =>
              `woml get ${runId} --state ${JSON.stringify(args.statePath)} --json`,
          })}`
        );
      }
    } catch {
      // Output replay remains useful even if final presentation was pruned.
    }
  };
  if (args.follow === false || streamClosed || descriptor === undefined) {
    finalPresentation();
    if (
      args.follow !== false &&
      !streamClosed &&
      descriptor === undefined &&
      !args.json
    ) {
      io.stderr(
        'Warning [WOML_LOG_RUNTIME_UNAVAILABLE]: Retained output was shown, but no active runtime is available to follow.\n'
      );
    }
    return 0;
  }

  if (!args.json) {
    io.stderr(
      `Following ${args.subjectKind} ${args.subject}. Press Ctrl+C to detach; workflows keep running.\n`
    );
  }
  let detached = false;
  let terminalReached = false;
  const deploymentId = descriptor.deploymentId;
  const abort = new AbortController();
  let resolveDetach!: () => void;
  const internalDetach = new Promise<void>(resolve => { resolveDetach = resolve; });
  const onSignal = (): void => resolveDetach();
  if (dependencies.waitForDetach === undefined) {
    process.once('SIGINT', onSignal);
    process.once('SIGTERM', onSignal);
  }
  void (dependencies.waitForDetach?.() ?? internalDetach).then(() => {
    detached = true;
    abort.abort();
  });
  const reconnectWindowMs = dependencies.reconnectWindowMs ?? 5_000;
  let failureStartedAt: number | undefined;
  try {
    while (!detached && !terminalReached) {
      try {
        const current = await readDescriptor(descriptorFile);
        if (current.deploymentId !== deploymentId) {
          throw new LogFollowError(
            'WOML_LOG_RUNTIME_UNAVAILABLE',
            'The runtime descriptor now belongs to a different deployment.'
          );
        }
        const query = new URLSearchParams({ after: String(cursor) });
        if (sources.size !== operationalSources.length) {
          query.set('source', [...sources].join(','));
        }
        const response = await fetcher(
          `${current.adminUrl}/v1/output/stream/${args.subjectKind === 'run' ? 'runs' : 'workflows'}/${encodeURIComponent(args.subject)}?${query}`,
          {
            headers: { authorization: `Bearer ${current.capability}` },
            signal: abort.signal,
          }
        );
        failureStartedAt = undefined;
        await consumeOperationalStream(
          response,
          event => {
            cursor = Math.max(cursor, event.cursor);
            if (event.event === 'record') {
              output(event.record);
              if (
                args.subjectKind === 'run' &&
                event.record.kind === 'closed'
              ) {
                terminalReached = true;
                abort.abort();
              }
            } else if (event.event === 'gap' && !args.json) {
              io.stderr(
                `Warning [WOML_OUTPUT_REPLAY_GAP]: Output before sequence ${event.gap.prunedThrough} is no longer retained.\n`
              );
            }
          },
          abort.signal
        );
        if (!detached && !terminalReached) {
          throw new Error('The operational-output stream closed.');
        }
      } catch (error) {
        if (detached || terminalReached || abort.signal.aborted) break;
        try { replay(); } catch { /* Retry through the live runtime. */ }
        failureStartedAt ??= Date.now();
        if (Date.now() - failureStartedAt >= reconnectWindowMs) {
          throw new LogFollowError(
            'WOML_LOG_RUNTIME_UNAVAILABLE',
            `The operational-output stream could not reconnect: ${error instanceof Error ? error.message : String(error)}`
          );
        }
        await Bun.sleep(100);
      }
    }
  } finally {
    clearInteractiveProgress();
    if (dependencies.waitForDetach === undefined) {
      process.off('SIGINT', onSignal);
      process.off('SIGTERM', onSignal);
    }
  }
  if (terminalReached) finalPresentation();
  return 0;
}

export async function followWorkflowLogs(options: {
  readonly args: LogFollowArguments;
  readonly io: LogFollowIo;
  readonly dependencies?: LogFollowerDependencies;
}): Promise<number> {
  const { args, io } = options;
  const dependencies = options.dependencies ?? {};
  if (args.operational === true) {
    return await followOperationalLogs({ args, io, dependencies });
  }
  const fetcher = dependencies.fetch ?? globalThis.fetch;
  const readDescriptor = dependencies.readDescriptor ?? readRuntimeDescriptor;
  const readRun =
    dependencies.readRun ??
    ((statePath: string, runId: string) =>
      inspectRunPresentationWithRust(statePath, runId, {
        nativeCorePath: dependencies.nativeCorePath,
      }));
  const readWorkflow =
    dependencies.readWorkflow ??
    ((statePath: string, workflowId: string, limit: number) =>
      listRunPresentationsWithRust(statePath, workflowId, limit, {
        nativeCorePath: dependencies.nativeCorePath,
      }));
  const hasWorkflow =
    dependencies.hasWorkflow ??
    ((statePath: string, workflowId: string) =>
      hasWorkflowDefinitionWithRust(statePath, workflowId, {
        nativeCorePath: dependencies.nativeCorePath,
      }));
  const render: PresentationRenderOptions = {
    format: args.json ? 'json' : io.isTTY === true ? 'tty' : 'plain',
    color: args.color,
    isTTY: io.isTTY === true,
    width: io.columns,
    environment: process.env,
    fullResultCommand: runId =>
      `woml get ${runId} --state ${JSON.stringify(args.statePath)} --json`,
  };
  const descriptorFile = runtimeDescriptorPath(args.statePath);
  const emitted = new Map<string, string>();
  let renderedAny = false;
  const emit = (presentation: RunPresentationV1): void => {
    const digest = JSON.stringify(presentation);
    if (emitted.get(presentation.runId) === digest) return;
    emitted.set(presentation.runId, digest);
    const output = renderRunPresentation(presentation, render);
    io.stdout(args.json || !renderedAny ? output : `\n${output}`);
    renderedAny = true;
  };
  const warning = (code: string, message: string): void => {
    if (args.json) {
      io.stderr(
        `Warning [${sanitizePresentationDiagnostic(code)}]: ${sanitizePresentationDiagnostic(message)}\n`
      );
    }
    else io.stderr(renderPresentationWarning(code, message, render));
  };

  let retainedRun: RunPresentationV1 | undefined;
  let retainedWorkflow: RunPresentationListV1 | undefined;
  let retainedWorkflowDefinition = false;
  const stateAvailable =
    dependencies.readRun !== undefined ||
    dependencies.readWorkflow !== undefined ||
    (await existingState(args.statePath));
  if (stateAvailable) {
    try {
      if (args.subjectKind === 'run') retainedRun = readRun(args.statePath, args.subject);
      else {
        retainedWorkflow = readWorkflow(args.statePath, args.subject, 10);
        retainedWorkflowDefinition = hasWorkflow(args.statePath, args.subject);
      }
    } catch (error) {
      if (stateAccessDenied(error)) throw stateAccessError(error);
      if (
        error instanceof RunManagementError &&
        error.code === 'WOML_RUN_STATE_UNAVAILABLE'
      ) {
        throw stateAccessError(error);
      }
      if (
        !(
          args.subjectKind === 'run' &&
          error instanceof RunManagementError &&
          error.code === 'WOML_RUN_NOT_FOUND'
        )
      ) {
        throw error;
      }
    }
  }
  if (retainedRun !== undefined) emit(retainedRun);
  if (retainedWorkflow !== undefined) {
    for (const run of [...retainedWorkflow.runs].sort((left, right) =>
      left.admittedAt.localeCompare(right.admittedAt)
    )) emit(run);
  }
  if (retainedRun !== undefined && terminal(retainedRun.status)) return 0;

  let descriptor: RuntimeDescriptorV1 | undefined;
  let snapshot: RuntimeSnapshot | undefined;
  let initialRuntimeError: unknown;
  try {
    descriptor = await readDescriptor(descriptorFile);
    snapshot = await runtimeSnapshot(descriptor, fetcher);
  } catch (error) {
    if (!inactiveDescriptor(error) && error instanceof RuntimeControlError) {
      if (error.code === 'WOML_RUNTIME_DESCRIPTOR_UNSAFE') throw error;
    }
    if (!inactiveDescriptor(error)) initialRuntimeError = error;
  }
  const activeSubject =
    args.subjectKind === 'workflow'
      ? snapshot?.workflows.includes(args.subject) === true
      : snapshot?.runs.some(run => run.runId === args.subject) === true;
  const hasRetained =
    retainedRun !== undefined ||
    retainedWorkflowDefinition ||
    (retainedWorkflow?.runs.length ?? 0) > 0;
  if (!hasRetained && !activeSubject) {
    if (descriptor !== undefined && initialRuntimeError !== undefined) {
      throw new LogFollowError(
        'WOML_LOG_RUNTIME_UNAVAILABLE',
        `The active runtime could not be queried: ${initialRuntimeError instanceof Error ? initialRuntimeError.message : String(initialRuntimeError)}`
      );
    }
    throw new LogFollowError(
      args.subjectKind === 'run'
        ? 'WOML_LOG_RUN_NOT_FOUND'
        : 'WOML_LOG_WORKFLOW_NOT_FOUND',
      args.subjectKind === 'run'
        ? `Run ${args.subject} has no retained history.`
        : `Workflow ${args.subject} is neither active nor present in retained run history.`
    );
  }
  if (descriptor === undefined) {
    warning(
      'WOML_LOG_RUNTIME_UNAVAILABLE',
      'Retained history was shown, but no active runtime is available to follow.'
    );
    return 0;
  }

  if (!args.json) {
    io.stderr(
      `Following ${args.subjectKind} ${args.subject}. Press Ctrl+C to detach; workflows keep running.\n`
    );
  }
  const deploymentId = descriptor.deploymentId;
  let sequence = snapshot?.sequence ?? 0;
  let runtimeInstanceId = snapshot?.runtimeInstanceId ?? descriptor.runtimeInstanceId;
  let terminalReached = false;
  let detached = false;
  let failureStartedAt: number | undefined;
  let refreshFailure: unknown;
  let interruptStream: (() => void) | undefined;
  const reconnectWindowMs = dependencies.reconnectWindowMs ?? 5_000;
  const abort = new AbortController();
  let refreshChain = Promise.resolve();
  const refresh = async (current: RuntimeDescriptorV1): Promise<void> => {
    if (args.subjectKind === 'run') {
      try {
        const value = decodeRunPresentationV1(
          JSON.stringify(
            await presentationResponse(
              current,
              `/v1/presentations/runs/${encodeURIComponent(args.subject)}`,
              fetcher,
              abort.signal
            )
          )
        );
        emit(value);
        if (terminal(value.status)) {
          terminalReached = true;
          abort.abort();
        }
      } catch (error) {
        if (
          error instanceof LogFollowError &&
          error.code === 'WOML_LOG_RUN_NOT_FOUND' &&
          emitted.has(args.subject)
        ) {
          warning(
            'WOML_LOG_HISTORY_PRUNED',
            `Retained history for ${args.subject} became unavailable while it was being followed.`
          );
          terminalReached = true;
          abort.abort();
          return;
        }
        throw error;
      }
      return;
    }
    const value = decodeRunPresentationListV1(
      JSON.stringify(
        await presentationResponse(
          current,
          `/v1/presentations/workflows/${encodeURIComponent(args.subject)}?limit=10`,
          fetcher,
          abort.signal
        )
      )
    );
    for (const run of [...value.runs].sort((left, right) =>
      left.admittedAt.localeCompare(right.admittedAt)
    )) emit(run);
  };
  const scheduleRefresh = (current: RuntimeDescriptorV1): void => {
    refreshChain = refreshChain
      .then(() => refresh(current))
      .catch(error => {
        refreshFailure = error;
        interruptStream?.();
      });
  };

  try {
    await refresh(descriptor);
  } catch {
    // A transient presentation failure is retried through the descriptor and
    // operations-stream reconnect loop below.
  }
  if (terminalReached) return 0;

  let resolveDetach!: () => void;
  const internalDetach = new Promise<void>(resolve => {
    resolveDetach = resolve;
  });
  const onSignal = (): void => resolveDetach();
  if (dependencies.waitForDetach === undefined) {
    process.once('SIGINT', onSignal);
    process.once('SIGTERM', onSignal);
  }
  const detach = dependencies.waitForDetach?.() ?? internalDetach;
  void detach.then(() => {
    detached = true;
    abort.abort();
  });

  try {
    while (!detached && !terminalReached) {
      try {
        const current = await readDescriptor(descriptorFile);
        if (current.deploymentId !== deploymentId) {
          throw new LogFollowError(
            'WOML_LOG_RUNTIME_UNAVAILABLE',
            'The runtime descriptor now belongs to a different deployment.'
          );
        }
        if (current.runtimeInstanceId !== runtimeInstanceId) {
          const currentSnapshot = await runtimeSnapshot(
            current,
            fetcher,
            abort.signal
          );
          runtimeInstanceId = currentSnapshot.runtimeInstanceId;
          sequence = currentSnapshot.sequence;
          await refresh(current);
        }
        const response = await fetcher(
          `${current.adminUrl}/v1/stream?after=${sequence}`,
          {
            headers: { authorization: `Bearer ${current.capability}` },
            signal: abort.signal,
          }
        );
        const connectionAbort = new AbortController();
        const closeConnection = (): void => connectionAbort.abort();
        abort.signal.addEventListener('abort', closeConnection, { once: true });
        if (abort.signal.aborted) connectionAbort.abort();
        interruptStream = closeConnection;
        failureStartedAt = undefined;
        try {
          await consumeOperationsStream(
            response,
            event => {
              if (event.runtimeInstanceId !== runtimeInstanceId) {
                throw new LogFollowError(
                  'WOML_LOG_RUNTIME_UNAVAILABLE',
                  'The operations stream identity does not match the runtime descriptor.'
                );
              }
              sequence = Math.max(sequence, event.sequence);
              if (event.subject.code === 'WOML_OBSERVABILITY_STREAM_GAP') {
                scheduleRefresh(current);
                return;
              }
              if (
                ['run', 'trigger', 'approval', 'retry', 'workflow_call', 'policy'].includes(
                  event.kind
                )
              ) {
                scheduleRefresh(current);
              }
            },
            connectionAbort.signal
          );
        } finally {
          interruptStream = undefined;
          abort.signal.removeEventListener('abort', closeConnection);
        }
        await refreshChain;
        if (refreshFailure !== undefined) {
          const error = refreshFailure;
          refreshFailure = undefined;
          throw error;
        }
        if (!detached && !terminalReached) {
          throw new Error('The active runtime stream closed.');
        }
      } catch (error) {
        await refreshChain.catch(() => {});
        if (detached || terminalReached || abort.signal.aborted) break;
        if (failureStartedAt === undefined) failureStartedAt = Date.now();
        if (Date.now() - failureStartedAt >= reconnectWindowMs) {
          warning(
            'WOML_LOG_RUNTIME_UNAVAILABLE',
            `Live following ended because the runtime is unavailable${error instanceof Error ? `: ${error.message}` : '.'}`
          );
          break;
        }
        await Bun.sleep(100);
      }
    }
  } finally {
    abort.abort();
    await refreshChain.catch(() => {});
    process.off('SIGINT', onSignal);
    process.off('SIGTERM', onSignal);
  }
  return 0;
}
