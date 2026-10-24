import {
  resolveOpenTelemetrySecrets,
  type OpenTelemetrySecretsV1,
  type ResolvedOpenTelemetryConfigurationV1,
} from './runtime-config';
import type {
  TelemetryAdmissionLinkV1,
  TelemetrySignalV1,
} from './opentelemetry-signal';
import {
  metricFactsForTelemetrySignal,
  prometheusIdentity,
  TelemetryActiveGaugeState,
} from './opentelemetry-observation';
import type { RuntimeTelemetryObservation } from './runtime-observability';

export {
  parseTelemetryAdmissionLink,
  parseTelemetrySignal,
} from './opentelemetry-signal';

export interface TelemetryObservationProjector {
  observe(signal: TelemetrySignalV1): RuntimeTelemetryObservation;
}

export function createTelemetryObservationProjector(): TelemetryObservationProjector {
  const active = new TelemetryActiveGaugeState();
  return {
    observe(signal) {
      active.observe(signal);
      return {
        signalId: signal.signalId,
        metrics: metricFactsForTelemetrySignal(signal).map(fact => ({
          identity: prometheusIdentity(fact),
          name: fact.name,
          instrument: fact.instrument,
          value: fact.value,
        })),
        gauges: active.facts().map(fact => ({
          name: fact.name,
          value: fact.value,
        })),
      };
    },
  };
}

export const OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE_V1 =
  'woml.opentelemetry-exporter-health/v1';
export const OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE =
  'woml.opentelemetry-exporter-health/v2';

export type OpenTelemetryRuntimeState =
  | 'disabled'
  | 'starting'
  | 'healthy'
  | 'degraded'
  | 'flushing'
  | 'stopped';

export interface OpenTelemetryExporterHealthV1 {
  readonly profile: typeof OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE_V1;
  readonly state: OpenTelemetryRuntimeState;
  readonly enabled: boolean;
  readonly protocol: 'http/protobuf';
  readonly signals: readonly ('traces' | 'metrics' | 'logs')[];
  readonly queue: {
    readonly size: number;
    readonly capacity: number;
  };
  readonly dropped: number;
  readonly lastErrorCode?: string;
}

export interface OpenTelemetryExporterHealthV2 {
  readonly profile: typeof OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE;
  readonly state: OpenTelemetryRuntimeState;
  readonly enabled: boolean;
  readonly protocol: 'http/protobuf';
  readonly signals: readonly ('traces' | 'metrics' | 'logs')[];
  readonly queue: {
    readonly size: number;
    readonly capacity: number;
    readonly utilization: number;
  };
  readonly exports: number;
  readonly failures: number;
  readonly timeouts: number;
  readonly dropped: number;
  readonly flushes: number;
  readonly consecutiveFailures: number;
  readonly circuit: 'closed' | 'open' | 'half_open';
  readonly lastSuccessAt?: string;
  readonly lastFailureAt?: string;
  readonly lastFlushAt?: string;
  readonly retryAt?: string;
  readonly lastErrorCode?: string;
}

export interface OpenTelemetryInspectorView {
  readonly lines: readonly string[];
  readonly colors: readonly (
    | 'cyan'
    | 'green'
    | 'yellow'
    | 'dim'
    | undefined
  )[];
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function count(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

export function openTelemetryInspectorView(
  value: unknown
): OpenTelemetryInspectorView | undefined {
  if (
    !record(value) ||
    value.profile !== 'woml.runtime-health/v2' ||
    !record(value.openTelemetry)
  ) {
    return undefined;
  }
  const health = value.openTelemetry;
  if (
    health.profile !== OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE ||
    !record(health.queue) ||
    !count(health.queue.size) ||
    !count(health.queue.capacity) ||
    typeof health.queue.utilization !== 'number' ||
    !Number.isFinite(health.queue.utilization) ||
    health.queue.utilization < 0 ||
    health.queue.utilization > 1 ||
    !count(health.exports) ||
    !count(health.failures) ||
    !count(health.timeouts) ||
    !count(health.dropped) ||
    !count(health.flushes) ||
    !['closed', 'open', 'half_open'].includes(String(health.circuit))
  ) {
    return undefined;
  }
  const retry = typeof health.retryAt === 'string' ? health.retryAt : undefined;
  const lastSuccess =
    typeof health.lastSuccessAt === 'string' ? health.lastSuccessAt : '—';
  const lastFlush =
    typeof health.lastFlushAt === 'string' ? health.lastFlushAt : '—';
  return {
    lines: [
      '',
      'OPENTELEMETRY EXPORT',
      `Queue: ${health.queue.size} / ${health.queue.capacity} (${Math.round(health.queue.utilization * 100)}%)`,
      `Exports: ${health.exports}  Failures: ${health.failures}  Timeouts: ${health.timeouts}  Dropped: ${health.dropped}`,
      `Circuit: ${health.circuit}${retry === undefined ? '' : `  Retry: ${retry}`}`,
      `Last success: ${lastSuccess}  Last flush: ${lastFlush}`,
    ],
    colors: [
      undefined,
      'cyan',
      health.queue.utilization >= 0.8 ? 'yellow' : 'green',
      health.failures > 0 || health.dropped > 0 ? 'yellow' : 'green',
      health.circuit === 'closed' ? 'green' : 'yellow',
      'dim',
    ],
  };
}

export type OpenTelemetryExporterEventKind =
  | 'export_succeeded'
  | 'export_failed'
  | 'export_timeout'
  | 'export_dropped'
  | 'retry_scheduled'
  | 'circuit_opened'
  | 'circuit_half_open'
  | 'circuit_closed';

export interface OpenTelemetryExporterEvent {
  readonly kind: OpenTelemetryExporterEventKind;
  readonly signal: 'traces' | 'metrics' | 'logs';
  readonly occurredAt: string;
  readonly code?: string;
  readonly retryAt?: string;
}

export interface OpenTelemetrySdkStartOptions {
  readonly configuration: ResolvedOpenTelemetryConfigurationV1;
  readonly secrets: OpenTelemetrySecretsV1;
  readonly resource: {
    readonly runtimeInstanceId: string;
    readonly deploymentId: string;
    readonly runtimeVersion: string;
  };
  readonly onExportEvent?: (event: OpenTelemetryExporterEvent) => void;
}

export interface OpenTelemetrySdkHandle {
  emit?(signal: TelemetrySignalV1): boolean;
  link?(link: TelemetryAdmissionLinkV1): boolean;
  forceFlush(): Promise<void>;
  shutdown(): Promise<void>;
}

export type OpenTelemetrySdkLoader = (
  options: OpenTelemetrySdkStartOptions
) => Promise<OpenTelemetrySdkHandle>;

export interface OpenTelemetryRuntimeOptions {
  readonly runtimeInstanceId: string;
  readonly deploymentId: string;
  readonly runtimeVersion: string;
  readonly environment?: Readonly<Record<string, string | undefined>>;
  readonly loadSdk?: OpenTelemetrySdkLoader;
  readonly onHealth?: (health: OpenTelemetryExporterHealthV2) => void;
  readonly onDiagnostic?: (code: string, message: string) => void;
  readonly now?: () => number;
}

async function loadBundledSdk(
  options: OpenTelemetrySdkStartOptions
): Promise<OpenTelemetrySdkHandle> {
  const modulePath = new URL(
    `opentelemetry-sdk.${import.meta.url.endsWith('.ts') ? 'ts' : 'js'}`,
    import.meta.url
  ).href;
  const module = (await import(modulePath)) as {
    startOpenTelemetrySdk(
      input: OpenTelemetrySdkStartOptions
    ): Promise<OpenTelemetrySdkHandle>;
  };
  return await module.startOpenTelemetrySdk(options);
}

function enabledSignals(
  configuration: ResolvedOpenTelemetryConfigurationV1
): readonly ('traces' | 'metrics' | 'logs')[] {
  return (['traces', 'metrics', 'logs'] as const).filter(
    signal => configuration.signals[signal]
  );
}

async function withinDeadline<T>(
  promise: Promise<T>,
  timeoutMs: number,
  code: string
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error(code)), timeoutMs);
        // This timer is the authority that bounds an operation which may never
        // settle. Keep it referenced until the race completes: Bun on Windows
        // may otherwise leave the awaited race pending indefinitely.
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

function exporterFailureCode(error: unknown, fallback: string): string {
  return error instanceof Error && /^WOML_OTEL_[A-Z0-9_]+$/u.test(error.message)
    ? error.message
    : fallback;
}

export class OpenTelemetryRuntime {
  readonly #configuration: ResolvedOpenTelemetryConfigurationV1;
  readonly #onHealth?: (health: OpenTelemetryExporterHealthV2) => void;
  readonly #onDiagnostic?: (code: string, message: string) => void;
  readonly #now: () => number;
  readonly #queue: (
    | { readonly kind: 'signal'; readonly value: TelemetrySignalV1 }
    | { readonly kind: 'link'; readonly value: TelemetryAdmissionLinkV1 }
  )[] = [];
  readonly #diagnosticAt = new Map<string, number>();
  #handle?: OpenTelemetrySdkHandle;
  #state: OpenTelemetryRuntimeState;
  #lastErrorCode?: string;
  #dropped = 0;
  #exports = 0;
  #failures = 0;
  #timeouts = 0;
  #flushes = 0;
  #consecutiveFailures = 0;
  #circuit: OpenTelemetryExporterHealthV2['circuit'] = 'closed';
  #lastSuccessAt?: string;
  #lastFailureAt?: string;
  #lastFlushAt?: string;
  #retryAt?: string;
  #drainScheduled = false;

  private constructor(
    configuration: ResolvedOpenTelemetryConfigurationV1,
    state: OpenTelemetryRuntimeState,
    options: Pick<OpenTelemetryRuntimeOptions, 'onHealth' | 'onDiagnostic' | 'now'>
  ) {
    this.#configuration = configuration;
    this.#state = state;
    this.#onHealth = options.onHealth;
    this.#onDiagnostic = options.onDiagnostic;
    this.#now = options.now ?? Date.now;
  }

  static async start(
    configuration: ResolvedOpenTelemetryConfigurationV1,
    options: OpenTelemetryRuntimeOptions
  ): Promise<OpenTelemetryRuntime> {
    const runtime = new OpenTelemetryRuntime(
      configuration,
      configuration.enabled ? 'starting' : 'disabled',
      options
    );
    runtime.#publish();
    if (!configuration.enabled) return runtime;

    try {
      const secrets = resolveOpenTelemetrySecrets(
        configuration,
        options.environment ?? process.env
      );
      runtime.#handle = await (options.loadSdk ?? loadBundledSdk)({
        configuration,
        secrets,
        resource: {
          runtimeInstanceId: options.runtimeInstanceId,
          deploymentId: options.deploymentId,
          runtimeVersion: options.runtimeVersion,
        },
        onExportEvent: event => runtime.#recordExportEvent(event),
      });
      runtime.#state = 'healthy';
    } catch {
      runtime.#state = 'degraded';
      runtime.#lastErrorCode = 'WOML_OTEL_EXPORTER_UNAVAILABLE';
      runtime.#lastFailureAt = new Date(runtime.#now()).toISOString();
      runtime.#failures += 1;
      runtime.#consecutiveFailures += 1;
      runtime.#diagnose(
        'WOML_OTEL_EXPORTER_UNAVAILABLE',
        'OpenTelemetry exporter startup failed; workflow execution remains available.'
      );
    }
    runtime.#publish();
    return runtime;
  }

  health(): OpenTelemetryExporterHealthV2 {
    const utilization =
      this.#queue.length / this.#configuration.maxQueueSize;
    return {
      profile: OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE,
      state: this.#state,
      enabled: this.#configuration.enabled,
      protocol: this.#configuration.protocol,
      signals: enabledSignals(this.#configuration),
      queue: {
        size: this.#queue.length,
        capacity: this.#configuration.maxQueueSize,
        utilization,
      },
      exports: this.#exports,
      failures: this.#failures,
      timeouts: this.#timeouts,
      dropped: this.#dropped,
      flushes: this.#flushes,
      consecutiveFailures: this.#consecutiveFailures,
      circuit: this.#circuit,
      ...(this.#lastSuccessAt === undefined ? {} : { lastSuccessAt: this.#lastSuccessAt }),
      ...(this.#lastFailureAt === undefined ? {} : { lastFailureAt: this.#lastFailureAt }),
      ...(this.#lastFlushAt === undefined ? {} : { lastFlushAt: this.#lastFlushAt }),
      ...(this.#retryAt === undefined ? {} : { retryAt: this.#retryAt }),
      ...(this.#lastErrorCode === undefined
        ? {}
        : { lastErrorCode: this.#lastErrorCode }),
    };
  }

  record(signal: TelemetrySignalV1): boolean {
    return this.#enqueue({ kind: 'signal', value: signal });
  }

  recordAdmissionLink(link: TelemetryAdmissionLinkV1): boolean {
    return this.#enqueue({ kind: 'link', value: link });
  }

  async shutdown(): Promise<void> {
    if (this.#state === 'stopped') return;
    if (!this.#configuration.enabled || this.#handle === undefined) {
      this.#state = 'stopped';
      this.#publish();
      return;
    }

    this.#state = 'flushing';
    this.#publish();
    this.#drainAll();
    const deadline = this.#now() + this.#configuration.shutdownTimeoutMs;
    try {
      await withinDeadline(
        this.#handle.forceFlush(),
        Math.max(0, deadline - this.#now()),
        'WOML_OTEL_FLUSH_TIMEOUT'
      );
      this.#flushes += 1;
      this.#lastFlushAt = new Date(this.#now()).toISOString();
    } catch (error) {
      this.#lastErrorCode = exporterFailureCode(
        error,
        'WOML_OTEL_FLUSH_FAILED'
      );
      if (this.#lastErrorCode === 'WOML_OTEL_FLUSH_TIMEOUT') this.#timeouts += 1;
      else this.#failures += 1;
      this.#lastFailureAt = new Date(this.#now()).toISOString();
      this.#diagnose(
        this.#lastErrorCode,
        'OpenTelemetry force-flush did not complete; runtime shutdown is continuing.'
      );
    }
    try {
      await withinDeadline(
        this.#handle.shutdown(),
        Math.max(0, deadline - this.#now()),
        'WOML_OTEL_SHUTDOWN_TIMEOUT'
      );
    } catch (error) {
      this.#lastErrorCode = exporterFailureCode(
        error,
        'WOML_OTEL_SHUTDOWN_FAILED'
      );
      if (this.#lastErrorCode === 'WOML_OTEL_SHUTDOWN_TIMEOUT') this.#timeouts += 1;
      else this.#failures += 1;
      this.#lastFailureAt = new Date(this.#now()).toISOString();
      this.#diagnose(
        this.#lastErrorCode,
        'OpenTelemetry shutdown did not complete before its deadline; runtime shutdown is continuing.'
      );
    }
    this.#handle = undefined;
    this.#state = 'stopped';
    this.#publish();
  }

  #publish(): void {
    this.#onHealth?.(this.health());
  }

  #enqueue(
    item:
      | { readonly kind: 'signal'; readonly value: TelemetrySignalV1 }
      | { readonly kind: 'link'; readonly value: TelemetryAdmissionLinkV1 }
  ): boolean {
    if (
      !this.#configuration.enabled ||
      this.#handle === undefined ||
      this.#state === 'starting' ||
      this.#state === 'flushing' ||
      this.#state === 'stopped'
    ) {
      return false;
    }
    if (this.#queue.length >= this.#configuration.maxQueueSize) {
      this.#dropped += 1;
      this.#lastErrorCode = 'WOML_OTEL_QUEUE_FULL';
      this.#diagnose(
        'WOML_OTEL_QUEUE_FULL',
        'OpenTelemetry queue capacity was reached; a telemetry fact was dropped.'
      );
      this.#publish();
      return false;
    }
    const wasEmpty = this.#queue.length === 0;
    this.#queue.push(item);
    if (wasEmpty) this.#publish();
    this.#scheduleDrain();
    return true;
  }

  #scheduleDrain(): void {
    if (this.#drainScheduled) return;
    this.#drainScheduled = true;
    queueMicrotask(() => {
      this.#drainScheduled = false;
      this.#drainBatch();
      if (this.#queue.length > 0) this.#scheduleDrain();
    });
  }

  #drainBatch(): void {
    const limit = Math.min(
      this.#configuration.maxExportBatchSize,
      this.#queue.length
    );
    const batch = this.#queue.splice(0, limit);
    for (const item of batch) {
      let accepted = false;
      try {
        accepted =
          item.kind === 'signal'
            ? (this.#handle?.emit?.(item.value) ?? false)
            : (this.#handle?.link?.(item.value) ?? false);
      } catch {
        accepted = false;
      }
      if (!accepted) {
        this.#dropped += 1;
        this.#lastErrorCode =
          item.kind === 'signal'
            ? 'WOML_OTEL_SIGNAL_REJECTED'
            : 'WOML_OTEL_CONTEXT_REJECTED';
        this.#diagnose(
          this.#lastErrorCode,
          'OpenTelemetry rejected a bounded telemetry fact; workflow execution is unaffected.'
        );
      }
    }
    this.#publish();
  }

  #drainAll(): void {
    while (this.#queue.length > 0) this.#drainBatch();
  }

  #recordExportEvent(event: OpenTelemetryExporterEvent): void {
    const occurredAt = Number.isFinite(Date.parse(event.occurredAt))
      ? event.occurredAt
      : new Date(this.#now()).toISOString();
    if (event.kind === 'export_succeeded') {
      this.#exports += 1;
      this.#consecutiveFailures = 0;
      this.#lastSuccessAt = occurredAt;
      this.#lastErrorCode = undefined;
      this.#retryAt = undefined;
      this.#circuit = 'closed';
      if (this.#state !== 'flushing' && this.#state !== 'stopped') {
        this.#state = 'healthy';
      }
    } else if (event.kind === 'export_failed' || event.kind === 'export_timeout') {
      this.#failures += 1;
      if (event.kind === 'export_timeout') this.#timeouts += 1;
      this.#consecutiveFailures += 1;
      this.#lastFailureAt = occurredAt;
      this.#lastErrorCode =
        event.code ??
        (event.kind === 'export_timeout'
          ? 'WOML_OTEL_EXPORT_TIMEOUT'
          : 'WOML_OTEL_EXPORT_FAILED');
      if (this.#state !== 'flushing' && this.#state !== 'stopped') {
        this.#state = 'degraded';
      }
      this.#diagnose(
        this.#lastErrorCode,
        event.kind === 'export_timeout'
          ? 'OpenTelemetry export timed out; workflow execution is continuing.'
          : 'OpenTelemetry export failed; workflow execution is continuing.'
      );
    } else if (event.kind === 'export_dropped') {
      this.#dropped += 1;
      this.#lastErrorCode = event.code ?? 'WOML_OTEL_EXPORT_DROPPED';
      this.#diagnose(
        this.#lastErrorCode,
        'OpenTelemetry dropped a batch while its collector circuit was open.'
      );
    } else if (event.kind === 'retry_scheduled') {
      this.#retryAt = event.retryAt;
    } else if (event.kind === 'circuit_opened') {
      this.#circuit = 'open';
      this.#retryAt = event.retryAt;
      if (this.#state !== 'flushing' && this.#state !== 'stopped') {
        this.#state = 'degraded';
      }
    } else if (event.kind === 'circuit_half_open') {
      this.#circuit = 'half_open';
    } else if (event.kind === 'circuit_closed') {
      this.#circuit = 'closed';
      this.#retryAt = undefined;
    }
    this.#publish();
  }

  #diagnose(code: string, message: string): void {
    const previous = this.#diagnosticAt.get(code) ?? Number.NEGATIVE_INFINITY;
    const current = this.#now();
    if (current - previous < 30_000) return;
    this.#diagnosticAt.set(code, current);
    this.#onDiagnostic?.(code, message);
  }
}
