import { sanitizeTerminalText, type ColorMode } from './terminal-presentation';

export const operationalSources = [
  'stdout',
  'stderr',
  'script',
  'runtime',
] as const;

export type OperationalSourceV1 = (typeof operationalSources)[number];
export type OperationalSubjectKind = 'run' | 'workflow';

interface OperationalRecordBaseV1 {
  readonly profile: 'woml.operational-record/v1';
  readonly deploymentId: string;
  readonly sequence: number;
  readonly occurredAt: string;
  readonly workflowId: string;
  readonly runId: string;
  readonly nodeId?: string;
  readonly attempt?: number;
  readonly invocationId?: string;
}

export type OperationalRecordV1 = OperationalRecordBaseV1 &
  (
    | {
        readonly kind: 'output';
        readonly source: OperationalSourceV1;
        readonly level?: 'debug' | 'info' | 'warn' | 'error';
        readonly text: string;
        readonly byteCount: number;
      }
    | {
        readonly kind: 'progress';
        readonly progressId: string;
        readonly current?: number;
        readonly total?: number;
        readonly unit?: string;
        readonly message?: string;
        readonly status: 'active' | 'closed';
      }
    | {
        readonly kind: 'truncated';
        readonly source: OperationalSourceV1;
        readonly droppedBytes: number;
        readonly reason: string;
      }
    | {
        readonly kind: 'artifact';
        readonly artifactId: string;
        readonly name: string;
        readonly size: number;
        readonly sha256: string;
        readonly contentType: string;
      }
    | {
        readonly kind: 'closed';
        readonly status:
          | 'succeeded'
          | 'failed'
          | 'cancelled'
          | 'timed_out'
          | 'interrupted';
        readonly reasonCode?: string;
      }
    | {
        readonly kind: 'gap';
        readonly requestedAfter: number;
        readonly prunedThrough: number;
        readonly earliestAvailable?: number;
      }
  );

export interface OperationalGapV1 {
  readonly requestedAfter: number;
  readonly prunedThrough: number;
  readonly earliestAvailable?: number;
}

export interface OperationalPageV1 {
  readonly profile: 'woml.operational-page/v1';
  readonly deploymentId: string;
  readonly runId?: string;
  readonly requestedAfter: number;
  readonly nextAfter: number;
  readonly earliestAvailable?: number;
  readonly latestSequence: number;
  readonly hasMore: boolean;
  readonly gap?: OperationalGapV1;
  readonly records: readonly OperationalRecordV1[];
}

export interface OperationalProgressSnapshotV1 {
  readonly runId: string;
  readonly nodeId?: string;
  readonly attempt?: number;
  readonly progressId: string;
  readonly current?: number;
  readonly total?: number;
  readonly unit?: string;
  readonly message?: string;
  readonly status: 'active' | 'closed';
  readonly sequence: number;
  readonly occurredAt: string;
}

export interface OperationalOutputObservationV1 {
  readonly profile: 'woml.operational-output-store/v1';
  readonly schemaVersion: number;
  readonly deploymentId: string;
  readonly nextSequence: number;
  readonly earliestAvailable?: number;
  readonly latestSequence: number;
  readonly prunedThrough: number;
  readonly recordCount: number;
  readonly retainedOutputBytes: number;
  readonly activeRuns: number;
  readonly terminalRuns: number;
  readonly truncatedStreams: number;
}

export interface OperationalOutputWakeupV1 {
  readonly profile: 'woml.operational-output-wakeup/v1';
  readonly newestSequence: number;
}

export type OperationalStreamEventV1 =
  | {
      readonly profile: 'woml.operational-stream/v1';
      readonly event: 'record';
      readonly cursor: number;
      readonly record: OperationalRecordV1;
    }
  | {
      readonly profile: 'woml.operational-stream/v1';
      readonly event: 'gap';
      readonly cursor: number;
      readonly gap: OperationalGapV1;
    }
  | {
      readonly profile: 'woml.operational-stream/v1';
      readonly event: 'heartbeat' | 'closed';
      readonly cursor: number;
    };

export class OperationalOutputError extends Error {
  readonly code: string;

  constructor(code: string, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'OperationalOutputError';
    this.code = code;
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function safeSequence(value: unknown, minimum = 0): value is number {
  return Number.isSafeInteger(value) && Number(value) >= minimum;
}

function boundedString(value: unknown, maximum = 320): value is string {
  return typeof value === 'string' && value.length > 0 && value.length <= maximum;
}

function identifier(value: unknown, maximum = 320): value is string {
  return boundedString(value, maximum) && !/\s/u.test(value);
}

function dateTime(value: unknown): value is string {
  return typeof value === 'string' && Number.isFinite(Date.parse(value));
}

function finiteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function optionalFiniteNumber(value: unknown): boolean {
  return value === undefined || finiteNumber(value);
}

function exactKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[] = []
): boolean {
  const allowed = new Set([...required, ...optional]);
  return (
    required.every(key => Object.hasOwn(value, key)) &&
    Object.keys(value).every(key => allowed.has(key))
  );
}

function decodeGap(value: unknown): OperationalGapV1 {
  if (
    !record(value) ||
    !exactKeys(value, ['requestedAfter', 'prunedThrough'], [
      'earliestAvailable',
    ]) ||
    !safeSequence(value.requestedAfter) ||
    !safeSequence(value.prunedThrough, 1) ||
    (value.earliestAvailable !== undefined &&
      !safeSequence(value.earliestAvailable, 1))
  ) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned an invalid operational-output gap.'
    );
  }
  return value as unknown as OperationalGapV1;
}

export function decodeOperationalRecordV1(value: unknown): OperationalRecordV1 {
  if (
    !record(value) ||
    value.profile !== 'woml.operational-record/v1' ||
    !identifier(value.deploymentId) ||
    !safeSequence(value.sequence, 1) ||
    !dateTime(value.occurredAt) ||
    !identifier(value.workflowId) ||
    !identifier(value.runId) ||
    (value.nodeId !== undefined && !identifier(value.nodeId)) ||
    (value.attempt !== undefined && !safeSequence(value.attempt, 1)) ||
    (value.invocationId !== undefined && !identifier(value.invocationId))
  ) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned an invalid operational-output record.'
    );
  }
  const identity = [
    'profile',
    'deploymentId',
    'sequence',
    'occurredAt',
    'workflowId',
    'runId',
    ...(value.nodeId === undefined ? [] : ['nodeId']),
    ...(value.attempt === undefined ? [] : ['attempt']),
    ...(value.invocationId === undefined ? [] : ['invocationId']),
    'kind',
  ];
  const source = value.source;
  switch (value.kind) {
    case 'output':
      if (
        !exactKeys(value, [...identity, 'source', 'text', 'byteCount'], ['level']) ||
        !operationalSources.includes(source as OperationalSourceV1) ||
        typeof value.text !== 'string' ||
        value.text.includes('\u0000') ||
        Buffer.byteLength(value.text) > 16 * 1024 ||
        !safeSequence(value.byteCount) ||
        Number(value.byteCount) > 16 * 1024 ||
        (value.level !== undefined &&
          !['debug', 'info', 'warn', 'error'].includes(String(value.level)))
      ) break;
      return value as unknown as OperationalRecordV1;
    case 'progress':
      if (
        !exactKeys(
          value,
          [...identity, 'progressId', 'status'],
          ['current', 'total', 'unit', 'message']
        ) ||
        !identifier(value.progressId, 128) ||
        !optionalFiniteNumber(value.current) ||
        !optionalFiniteNumber(value.total) ||
        (value.current === undefined && value.message === undefined) ||
        (value.total !== undefined && Number(value.total) <= 0) ||
        (value.current !== undefined &&
          value.total !== undefined &&
          Number(value.current) > Number(value.total)) ||
        (value.unit !== undefined && value.current === undefined) ||
        (value.unit !== undefined && !boundedString(value.unit, 32)) ||
        (value.message !== undefined && !boundedString(value.message, 512)) ||
        !['active', 'closed'].includes(String(value.status))
      ) break;
      return value as unknown as OperationalRecordV1;
    case 'truncated':
      if (
        !exactKeys(value, [...identity, 'source', 'droppedBytes', 'reason']) ||
        !operationalSources.includes(source as OperationalSourceV1) ||
        !safeSequence(value.droppedBytes, 1) ||
        !boundedString(value.reason, 128)
      ) break;
      return value as unknown as OperationalRecordV1;
    case 'artifact':
      if (
        !exactKeys(value, [
          ...identity,
          'artifactId',
          'name',
          'size',
          'sha256',
          'contentType',
        ]) ||
        !identifier(value.artifactId) ||
        !boundedString(value.name, 256) ||
        !safeSequence(value.size) ||
        typeof value.sha256 !== 'string' ||
        !/^[0-9a-f]{64}$/.test(value.sha256) ||
        !boundedString(value.contentType, 255)
      ) break;
      return value as unknown as OperationalRecordV1;
    case 'closed':
      if (
        !exactKeys(value, [...identity, 'status'], ['reasonCode']) ||
        !['succeeded', 'failed', 'cancelled', 'timed_out', 'interrupted'].includes(
          String(value.status)
        ) ||
        (value.reasonCode !== undefined &&
          (typeof value.reasonCode !== 'string' ||
            !/^WOML_[A-Z0-9_]+$/.test(value.reasonCode) ||
            value.reasonCode.length > 128))
      ) break;
      return value as unknown as OperationalRecordV1;
    case 'gap':
      if (
        !exactKeys(
          value,
          [...identity, 'requestedAfter', 'prunedThrough'],
          ['earliestAvailable']
        ) ||
        !safeSequence(value.requestedAfter) ||
        !safeSequence(value.prunedThrough, 1) ||
        (value.earliestAvailable !== undefined &&
          !safeSequence(value.earliestAvailable, 1))
      ) break;
      return value as unknown as OperationalRecordV1;
  }
  throw new OperationalOutputError(
    'WOML_OUTPUT_PROTOCOL_INVALID',
    'The native core returned an invalid operational-output record body.'
  );
}

export function decodeOperationalPageV1(json: string): OperationalPageV1 {
  const value: unknown = JSON.parse(json);
  if (
    !record(value) ||
    !exactKeys(
      value,
      [
        'profile',
        'deploymentId',
        'requestedAfter',
        'nextAfter',
        'latestSequence',
        'hasMore',
        'records',
      ],
      ['runId', 'earliestAvailable', 'gap']
    ) ||
    value.profile !== 'woml.operational-page/v1' ||
    !identifier(value.deploymentId) ||
    (value.runId !== undefined && !identifier(value.runId)) ||
    !safeSequence(value.requestedAfter) ||
    !safeSequence(value.nextAfter) ||
    (value.earliestAvailable !== undefined &&
      !safeSequence(value.earliestAvailable, 1)) ||
    !safeSequence(value.latestSequence) ||
    typeof value.hasMore !== 'boolean' ||
    !Array.isArray(value.records) ||
    value.records.length > 1_000
  ) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned an invalid operational-output page.'
    );
  }
  const records = value.records.map(decodeOperationalRecordV1);
  const gap = value.gap === undefined ? undefined : decodeGap(value.gap);
  if (
    Number(value.nextAfter) < Number(value.requestedAfter) ||
    records.some((item, index) =>
      item.sequence <= Number(value.requestedAfter) ||
      (index > 0 && item.sequence <= records[index - 1]!.sequence)
    ) ||
    (records.at(-1)?.sequence ?? Number(value.nextAfter)) >
      Number(value.nextAfter)
  ) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned an unordered operational-output page.'
    );
  }
  return {
    ...(value as unknown as OperationalPageV1),
    records,
    ...(gap === undefined ? {} : { gap }),
  };
}

export function decodeOperationalProgressV1(
  json: string
): readonly OperationalProgressSnapshotV1[] {
  const value: unknown = JSON.parse(json);
  if (!Array.isArray(value) || value.length > 1_000) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned invalid operational progress.'
    );
  }
  for (const item of value) {
    if (
      !record(item) ||
      !exactKeys(
        item,
        ['runId', 'progressId', 'status', 'sequence', 'occurredAt'],
        ['nodeId', 'attempt', 'current', 'total', 'unit', 'message']
      ) ||
      !identifier(item.runId) ||
      (item.nodeId !== undefined && !identifier(item.nodeId)) ||
      (item.attempt !== undefined && !safeSequence(item.attempt, 1)) ||
      !identifier(item.progressId, 128) ||
      !optionalFiniteNumber(item.current) ||
      !optionalFiniteNumber(item.total) ||
      (item.current === undefined && item.message === undefined) ||
      (item.total !== undefined && Number(item.total) <= 0) ||
      (item.current !== undefined &&
        item.total !== undefined &&
        Number(item.current) > Number(item.total)) ||
      (item.unit !== undefined && item.current === undefined) ||
      (item.unit !== undefined && !boundedString(item.unit, 32)) ||
      (item.message !== undefined && !boundedString(item.message, 512)) ||
      !['active', 'closed'].includes(String(item.status)) ||
      !safeSequence(item.sequence, 1) ||
      !dateTime(item.occurredAt)
    ) {
      throw new OperationalOutputError(
        'WOML_OUTPUT_PROTOCOL_INVALID',
        'The native core returned invalid operational progress.'
      );
    }
  }
  return value as readonly OperationalProgressSnapshotV1[];
}

export function decodeOperationalObservationV1(
  json: string
): OperationalOutputObservationV1 {
  const value: unknown = JSON.parse(json);
  const fields = [
    'schemaVersion',
    'nextSequence',
    'latestSequence',
    'prunedThrough',
    'recordCount',
    'retainedOutputBytes',
    'activeRuns',
    'terminalRuns',
    'truncatedStreams',
  ] as const;
  if (
    !record(value) ||
    !exactKeys(
      value,
      ['profile', 'deploymentId', ...fields],
      ['earliestAvailable']
    ) ||
    value.profile !== 'woml.operational-output-store/v1' ||
    !identifier(value.deploymentId) ||
    !fields.every(field => safeSequence(value[field])) ||
    (value.earliestAvailable !== undefined &&
      !safeSequence(value.earliestAvailable, 1))
  ) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned invalid operational-output observation data.'
    );
  }
  return value as unknown as OperationalOutputObservationV1;
}

export function parseOperationalOutputWakeup(
  json: string
): OperationalOutputWakeupV1 {
  const value: unknown = JSON.parse(json);
  if (
    !record(value) ||
    !exactKeys(value, ['profile', 'newestSequence']) ||
    value.profile !== 'woml.operational-output-wakeup/v1' ||
    !safeSequence(value.newestSequence, 1)
  ) {
    throw new OperationalOutputError(
      'WOML_OUTPUT_PROTOCOL_INVALID',
      'The native core returned an invalid operational-output wakeup.'
    );
  }
  return value as unknown as OperationalOutputWakeupV1;
}

function streamEvent(value: OperationalStreamEventV1): Uint8Array {
  return new TextEncoder().encode(
    `id: ${value.cursor}\nevent: ${value.event}\ndata: ${JSON.stringify(value)}\n\n`
  );
}

export interface RuntimeOperationalOutputSurface {
  page(request: {
    readonly subjectKind: OperationalSubjectKind;
    readonly subject: string;
    readonly after: number;
    readonly limit: number;
    readonly sources: ReadonlySet<OperationalSourceV1>;
  }): OperationalPageV1 | Promise<OperationalPageV1>;
  progress(runId: string):
    | readonly OperationalProgressSnapshotV1[]
    | Promise<readonly OperationalProgressSnapshotV1[]>;
  observe(): OperationalOutputObservationV1 | Promise<OperationalOutputObservationV1>;
  stream(request: {
    readonly subjectKind: OperationalSubjectKind;
    readonly subject: string;
    readonly after: number;
    readonly sources: ReadonlySet<OperationalSourceV1>;
  }): Response | Promise<Response>;
  notify(wakeup: OperationalOutputWakeupV1): void;
  closeStreams(): void;
  readonly followerCount: number;
}

interface Follower {
  readonly subjectKind: OperationalSubjectKind;
  readonly subject: string;
  readonly sources: ReadonlySet<OperationalSourceV1>;
  readonly controller: ReadableStreamDefaultController<Uint8Array>;
  cursor: number;
  flushing: boolean;
  pending: boolean;
  closed: boolean;
}

export class RuntimeOperationalOutput implements RuntimeOperationalOutputSurface {
  readonly #readPage: (
    runId: string | undefined,
    after: number,
    limit: number
  ) => OperationalPageV1;
  readonly #readProgress: (runId: string) => readonly OperationalProgressSnapshotV1[];
  readonly #readObservation: () => OperationalOutputObservationV1;
  readonly #deploymentId?: string;
  readonly #hasRun?: (runId: string) => boolean;
  readonly #hasWorkflow?: (workflowId: string) => boolean;
  readonly #validatedSubjects = new Set<string>();
  readonly #followers = new Set<Follower>();
  readonly #heartbeat: ReturnType<typeof setInterval>;
  #newestSequence = 0;

  constructor(options: {
    readonly readPage: (
      runId: string | undefined,
      after: number,
      limit: number
    ) => OperationalPageV1;
    readonly readProgress: (
      runId: string
    ) => readonly OperationalProgressSnapshotV1[];
    readonly readObservation: () => OperationalOutputObservationV1;
    readonly deploymentId?: string;
    readonly hasRun?: (runId: string) => boolean;
    readonly hasWorkflow?: (workflowId: string) => boolean;
    readonly heartbeatMs?: number;
  }) {
    this.#readPage = options.readPage;
    this.#readProgress = options.readProgress;
    this.#readObservation = options.readObservation;
    this.#deploymentId = options.deploymentId;
    this.#hasRun = options.hasRun;
    this.#hasWorkflow = options.hasWorkflow;
    this.#heartbeat = setInterval(
      () => this.#emitHeartbeats(),
      options.heartbeatMs ?? 15_000
    );
    this.#heartbeat.unref?.();
  }

  get followerCount(): number {
    return this.#followers.size;
  }

  page(request: {
    readonly subjectKind: OperationalSubjectKind;
    readonly subject: string;
    readonly after: number;
    readonly limit: number;
    readonly sources: ReadonlySet<OperationalSourceV1>;
  }): OperationalPageV1 {
    this.#validateSubject(request.subjectKind, request.subject);
    const page = this.#readPage(
      request.subjectKind === 'run' ? request.subject : undefined,
      request.after,
      request.limit
    );
    this.#validateDeployment(page.deploymentId);
    const records = page.records.filter(item =>
      (request.subjectKind === 'run'
        ? item.runId === request.subject
        : item.workflowId === request.subject) &&
      (item.kind !== 'output' && item.kind !== 'truncated'
        ? true
        : request.sources.has(item.source))
    );
    return { ...page, records };
  }

  progress(runId: string): readonly OperationalProgressSnapshotV1[] {
    this.#validateSubject('run', runId);
    return this.#readProgress(runId);
  }

  observe(): OperationalOutputObservationV1 {
    const observation = this.#readObservation();
    this.#validateDeployment(observation.deploymentId);
    return observation;
  }

  stream(request: {
    readonly subjectKind: OperationalSubjectKind;
    readonly subject: string;
    readonly after: number;
    readonly sources: ReadonlySet<OperationalSourceV1>;
  }): Response {
    this.#validateSubject(request.subjectKind, request.subject);
    if (this.#followers.size >= 8) {
      throw new OperationalOutputError(
        'WOML_OUTPUT_FOLLOWER_LIMIT',
        'The runtime already has eight operational-output followers.'
      );
    }
    let follower: Follower;
    const body = new ReadableStream<Uint8Array>(
      {
        start: controller => {
          follower = {
            ...request,
            cursor: request.after,
            controller,
            flushing: false,
            pending: false,
            closed: false,
          };
          this.#followers.add(follower);
          this.#scheduleFlush(follower);
        },
        cancel: () => {
          if (follower !== undefined) this.#removeFollower(follower);
        },
      },
      { highWaterMark: 256 }
    );
    return new Response(body, {
      headers: {
        'content-type': 'text/event-stream; charset=utf-8',
        'cache-control': 'no-store',
        connection: 'keep-alive',
        'x-accel-buffering': 'no',
      },
    });
  }

  notify(wakeup: OperationalOutputWakeupV1): void {
    if (wakeup.newestSequence <= this.#newestSequence) return;
    this.#newestSequence = wakeup.newestSequence;
    for (const follower of this.#followers) this.#scheduleFlush(follower);
  }

  closeStreams(): void {
    for (const follower of [...this.#followers]) {
      this.#enqueue(follower, {
        profile: 'woml.operational-stream/v1',
        event: 'closed',
        cursor: follower.cursor,
      });
      this.#removeFollower(follower, true);
    }
  }

  #scheduleFlush(follower: Follower): void {
    if (follower.closed) return;
    if (follower.flushing) {
      follower.pending = true;
      return;
    }
    follower.flushing = true;
    queueMicrotask(() => this.#flush(follower));
  }

  #flush(follower: Follower): void {
    try {
      let pages = 0;
      do {
        follower.pending = false;
        const page = this.page({
          subjectKind: follower.subjectKind,
          subject: follower.subject,
          after: follower.cursor,
          limit: 250,
          sources: follower.sources,
        });
        if (page.gap !== undefined) {
          this.#enqueue(follower, {
            profile: 'woml.operational-stream/v1',
            event: 'gap',
            cursor: page.nextAfter,
            gap: page.gap,
          });
        }
        for (const item of page.records) {
          this.#enqueue(follower, {
            profile: 'woml.operational-stream/v1',
            event: 'record',
            cursor: item.sequence,
            record: item,
          });
          if (follower.closed) return;
        }
        follower.cursor = Math.max(follower.cursor, page.nextAfter);
        pages += 1;
        if (!page.hasMore || pages >= 4) break;
      } while (!follower.closed);
    } catch {
      this.#removeFollower(follower, true);
      return;
    } finally {
      follower.flushing = false;
    }
    if (follower.pending && !follower.closed) this.#scheduleFlush(follower);
  }

  #enqueue(follower: Follower, event: OperationalStreamEventV1): void {
    if (follower.closed) return;
    if ((follower.controller.desiredSize ?? 0) <= 0) {
      this.#removeFollower(follower, true);
      return;
    }
    follower.controller.enqueue(streamEvent(event));
  }

  #emitHeartbeats(): void {
    for (const follower of [...this.#followers]) {
      this.#enqueue(follower, {
        profile: 'woml.operational-stream/v1',
        event: 'heartbeat',
        cursor: follower.cursor,
      });
    }
  }

  #removeFollower(follower: Follower, close = false): void {
    if (follower.closed) return;
    follower.closed = true;
    this.#followers.delete(follower);
    if (close) {
      try {
        follower.controller.close();
      } catch {
        // The client may already have cancelled the stream.
      }
    }
  }

  #validateDeployment(deploymentId: string): void {
    if (this.#deploymentId !== undefined && deploymentId !== this.#deploymentId) {
      throw new OperationalOutputError(
        'WOML_OUTPUT_STORE_DEPLOYMENT_MISMATCH',
        'The operational-output store belongs to another deployment.'
      );
    }
  }

  #validateSubject(kind: OperationalSubjectKind, subject: string): void {
    const key = `${kind}:${subject}`;
    if (this.#validatedSubjects.has(key)) return;
    const exists = kind === 'run'
      ? this.#hasRun?.(subject)
      : this.#hasWorkflow?.(subject);
    if (exists === false) {
      throw new OperationalOutputError(
        'WOML_OUTPUT_SUBJECT_NOT_FOUND',
        `The ${kind} operational-output subject does not exist.`
      );
    }
    this.#validatedSubjects.add(key);
  }
}

export function renderOperationalRecord(
  item: OperationalRecordV1,
  options: {
    readonly color: ColorMode;
    readonly includeRunLabel: boolean;
    readonly unicode: boolean;
    readonly isTTY?: boolean;
    readonly width?: number;
  }
): string {
  const colorEnabled =
    options.color === 'always' ||
    (options.color === 'auto' &&
      process.env.NO_COLOR === undefined &&
      (options.isTTY ?? process.stderr.isTTY) === true);
  const paint = (value: string, code: number): string =>
    colorEnabled ? `\u001b[${code}m${value}\u001b[0m` : value;
  const fit = (value: string): string => {
    const width = options.width;
    if (width === undefined || width < 8 || value.length <= width) return value;
    const tokens = value.match(/\u001b\[[0-9;]*m|./gu) ?? [];
    let visible = 0;
    let result = '';
    for (const token of tokens) {
      if (token.startsWith('\u001b[')) {
        result += token;
        continue;
      }
      if (visible >= width - 1) {
        return `${result}${colorEnabled ? '\u001b[0m' : ''}${options.unicode ? '…' : '.'}`;
      }
      result += token;
      visible += 1;
    }
    return result;
  };
  const label = [
    ...(options.includeRunLabel ? [item.runId] : []),
    ...(item.nodeId === undefined ? [] : [item.nodeId]),
  ]
    .map(value => sanitizeTerminalText(value).replaceAll('\n', ' '))
    .join(' · ');
  const prefix = label.length === 0 ? '' : `[${label}] `;
  if (item.kind === 'output') {
    const source = item.source === 'script' && item.level !== undefined
      ? `${item.source}:${item.level}`
      : item.source;
    const sourceColor =
      item.source === 'stderr' || item.level === 'error'
        ? 31
        : item.level === 'warn'
          ? 33
          : item.source === 'script'
            ? 36
            : 32;
    return sanitizeTerminalText(item.text)
      .split('\n')
      .map(line => fit(`${prefix}${paint(source, sourceColor)}  ${line}`))
      .join('\n');
  }
  if (item.kind === 'progress') {
    const amount = item.current === undefined
      ? ''
      : item.total === undefined
        ? `${item.current}${item.unit === undefined ? '' : ` ${item.unit}`}`
        : `${Math.min(100, Math.floor((item.current / item.total) * 100))}% · ${item.current} of ${item.total}${item.unit === undefined ? '' : ` ${item.unit}`}`;
    const message = item.message === undefined ? '' : sanitizeTerminalText(item.message);
    return fit(`${prefix}${paint(options.unicode ? '◌' : '*', 36)} ${[amount, message].filter(Boolean).join(' · ')}`);
  }
  if (item.kind === 'truncated') {
    return fit(`${prefix}${paint('!', 33)} ${item.source} output truncated · ${item.droppedBytes} bytes · ${sanitizeTerminalText(item.reason)}`);
  }
  if (item.kind === 'artifact') {
    return fit(`${prefix}${paint(options.unicode ? '↳' : '->', 36)} output saved as artifact ${sanitizeTerminalText(item.name)}`);
  }
  if (item.kind === 'closed') {
    return fit(`${prefix}${paint(options.unicode ? '✓' : '+', item.status === 'succeeded' ? 32 : 31)} stream ${item.status.replaceAll('_', ' ')}`);
  }
  return fit(`${prefix}${paint('!', 33)} output history has a gap through sequence ${item.prunedThrough}`);
}
