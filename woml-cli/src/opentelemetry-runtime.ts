import {
  resolveOpenTelemetrySecrets,
  type OpenTelemetrySecretsV1,
  type ResolvedOpenTelemetryConfigurationV1,
} from './runtime-config';
import type {
  TelemetryAdmissionLinkV1,
  TelemetrySignalV1,
} from './opentelemetry-signal';

export const OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE =
  'woml.opentelemetry-exporter-health/v1';

export type OpenTelemetryRuntimeState =
  | 'disabled'
  | 'starting'
  | 'healthy'
  | 'degraded'
  | 'flushing'
  | 'stopped';

export interface OpenTelemetryExporterHealthV1 {
  readonly profile: typeof OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE;
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

export interface OpenTelemetrySdkStartOptions {
  readonly configuration: ResolvedOpenTelemetryConfigurationV1;
  readonly secrets: OpenTelemetrySecretsV1;
  readonly resource: {
    readonly runtimeInstanceId: string;
    readonly deploymentId: string;
    readonly runtimeVersion: string;
  };
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
  readonly onHealth?: (health: OpenTelemetryExporterHealthV1) => void;
}

async function loadBundledSdk(
  options: OpenTelemetrySdkStartOptions
): Promise<OpenTelemetrySdkHandle> {
  const modulePath = import.meta.url.endsWith('.ts')
    ? './opentelemetry-sdk.ts'
    : './opentelemetry-sdk.js';
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
        timer.unref?.();
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
  readonly #onHealth?: (health: OpenTelemetryExporterHealthV1) => void;
  #handle?: OpenTelemetrySdkHandle;
  #state: OpenTelemetryRuntimeState;
  #lastErrorCode?: string;
  #dropped = 0;

  private constructor(
    configuration: ResolvedOpenTelemetryConfigurationV1,
    state: OpenTelemetryRuntimeState,
    onHealth?: (health: OpenTelemetryExporterHealthV1) => void
  ) {
    this.#configuration = configuration;
    this.#state = state;
    this.#onHealth = onHealth;
  }

  static async start(
    configuration: ResolvedOpenTelemetryConfigurationV1,
    options: OpenTelemetryRuntimeOptions
  ): Promise<OpenTelemetryRuntime> {
    const runtime = new OpenTelemetryRuntime(
      configuration,
      configuration.enabled ? 'starting' : 'disabled',
      options.onHealth
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
      });
      runtime.#state = 'healthy';
    } catch {
      runtime.#state = 'degraded';
      runtime.#lastErrorCode = 'WOML_OTEL_EXPORTER_UNAVAILABLE';
    }
    runtime.#publish();
    return runtime;
  }

  health(): OpenTelemetryExporterHealthV1 {
    return {
      profile: OPEN_TELEMETRY_EXPORTER_HEALTH_PROFILE,
      state: this.#state,
      enabled: this.#configuration.enabled,
      protocol: this.#configuration.protocol,
      signals: enabledSignals(this.#configuration),
      queue: {
        size: 0,
        capacity: this.#configuration.maxQueueSize,
      },
      dropped: this.#dropped,
      ...(this.#lastErrorCode === undefined
        ? {}
        : { lastErrorCode: this.#lastErrorCode }),
    };
  }

  record(signal: TelemetrySignalV1): boolean {
    if (
      this.#state !== 'healthy' ||
      this.#handle === undefined ||
      this.#handle.emit === undefined
    ) {
      return false;
    }
    try {
      const accepted = this.#handle.emit(signal);
      if (!accepted) {
        this.#dropped += 1;
        this.#publish();
      }
      return accepted;
    } catch {
      this.#dropped += 1;
      this.#lastErrorCode = 'WOML_OTEL_SIGNAL_REJECTED';
      this.#publish();
      return false;
    }
  }

  recordAdmissionLink(link: TelemetryAdmissionLinkV1): boolean {
    if (
      this.#state !== 'healthy' ||
      this.#handle === undefined ||
      this.#handle.link === undefined
    ) {
      return false;
    }
    try {
      const accepted = this.#handle.link(link);
      if (!accepted) {
        this.#dropped += 1;
        this.#publish();
      }
      return accepted;
    } catch {
      this.#dropped += 1;
      this.#lastErrorCode = 'WOML_OTEL_CONTEXT_REJECTED';
      this.#publish();
      return false;
    }
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
    const deadline = Date.now() + this.#configuration.shutdownTimeoutMs;
    try {
      await withinDeadline(
        this.#handle.forceFlush(),
        Math.max(0, deadline - Date.now()),
        'WOML_OTEL_FLUSH_TIMEOUT'
      );
    } catch (error) {
      this.#lastErrorCode = exporterFailureCode(
        error,
        'WOML_OTEL_FLUSH_FAILED'
      );
    }
    const remainingMs = deadline - Date.now();
    if (remainingMs > 0) {
      try {
        await withinDeadline(
          this.#handle.shutdown(),
          remainingMs,
          'WOML_OTEL_SHUTDOWN_TIMEOUT'
        );
      } catch (error) {
        this.#lastErrorCode = exporterFailureCode(
          error,
          'WOML_OTEL_SHUTDOWN_FAILED'
        );
      }
    }
    this.#handle = undefined;
    this.#state = 'stopped';
    this.#publish();
  }

  #publish(): void {
    this.#onHealth?.(this.health());
  }
}
