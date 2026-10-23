import { constants } from 'node:fs';
import { access, stat, statfs } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

export const RUNTIME_CONFIG_PROFILE_V1 = 'woml.runtime-configuration/v1';
export const RUNTIME_CONFIG_PROFILE_V2 = 'woml.runtime-configuration/v2';
// Kept for source compatibility with callers that consume the frozen v1 name.
export const RUNTIME_CONFIG_PROFILE = RUNTIME_CONFIG_PROFILE_V1;
export const RUNTIME_CONFIG_MAX_BYTES = 64 * 1024;
export const RUNTIME_MIN_FREE_BYTES = 64 * 1024 * 1024;

export const OPEN_TELEMETRY_CONFIGURATION_PROFILE =
  'woml.opentelemetry-configuration/v1';
export const OPEN_TELEMETRY_DEFAULT_ENDPOINT = 'http://127.0.0.1:4318';
export const OPEN_TELEMETRY_DEFAULT_QUEUE_SIZE = 2_048;
export const OPEN_TELEMETRY_MAX_QUEUE_SIZE = 65_536;
export const OPEN_TELEMETRY_DEFAULT_BATCH_SIZE = 512;
export const OPEN_TELEMETRY_MAX_BATCH_SIZE = 1_024;
export const OPEN_TELEMETRY_DEFAULT_EXPORT_INTERVAL_MS = 10_000;
export const OPEN_TELEMETRY_DEFAULT_EXPORT_TIMEOUT_MS = 5_000;
export const OPEN_TELEMETRY_DEFAULT_SHUTDOWN_TIMEOUT_MS = 5_000;

const hostPattern = /^(?:localhost|(?:[a-zA-Z0-9](?:[a-zA-Z0-9.-]{0,251}[a-zA-Z0-9])?)|(?:\d{1,3}\.){3}\d{1,3}|[0-9a-fA-F:]+)$/;
const deploymentPattern = /^[a-z][a-z0-9]*(?:[._-][a-z0-9]+)*$/;

export interface RuntimeConfigurationV1 {
  readonly schemaVersion: 1;
  readonly deploymentName?: string;
  readonly statePath?: string;
  readonly public?: {
    readonly host?: string;
    readonly port?: number;
  };
  readonly admin?: {
    readonly host?: string;
    readonly port?: number;
  };
  readonly logging?: {
    readonly format?: 'text' | 'json';
    readonly level?: 'error' | 'warn' | 'info' | 'debug';
    readonly directory?: string;
  };
  readonly workers?: number;
  readonly shutdownTimeoutMs?: number;
  readonly observability?: {
    readonly health?: boolean;
    readonly metrics?: boolean;
  };
  readonly retention?: {
    readonly enabled: boolean;
    readonly succeededAfterDays?: number;
    readonly failedAfterDays?: number;
    readonly cancelledAfterDays?: number;
    readonly maintenanceHourUtc?: number;
  };
  readonly backup?: {
    readonly directory?: string;
  };
}

export interface OpenTelemetryConfigurationV1 {
  readonly enabled?: boolean;
  readonly protocol?: 'http/protobuf';
  readonly traces?: boolean;
  readonly metrics?: boolean;
  readonly logs?: boolean;
  readonly samplingRatio?: number;
  readonly exportIntervalMs?: number;
  readonly exportTimeoutMs?: number;
  readonly shutdownTimeoutMs?: number;
  readonly maxQueueSize?: number;
  readonly maxExportBatchSize?: number;
  readonly propagation?: boolean;
}

export interface RuntimeConfigurationV2
  extends Omit<RuntimeConfigurationV1, 'schemaVersion' | 'observability'> {
  readonly schemaVersion: 2;
  readonly observability?: {
    readonly health?: boolean;
    readonly metrics?: boolean;
    readonly openTelemetry?: OpenTelemetryConfigurationV1;
  };
}

export type RuntimeConfiguration = RuntimeConfigurationV1 | RuntimeConfigurationV2;

export interface RuntimeConfigurationOverrides {
  readonly deploymentName?: string;
  readonly statePath?: string;
  readonly publicHost?: string;
  readonly publicPort?: number;
  readonly adminHost?: string;
  readonly adminPort?: number;
  readonly logFormat?: 'text' | 'json';
  readonly logLevel?: 'error' | 'warn' | 'info' | 'debug';
  readonly logDirectory?: string;
  readonly workers?: number;
  readonly shutdownTimeoutMs?: number;
}

export interface ResolvedRuntimeConfigurationV1 {
  readonly profile: typeof RUNTIME_CONFIG_PROFILE_V1;
  readonly schemaVersion: 1;
  readonly configPath?: string;
  readonly deploymentName: string;
  readonly statePath: string;
  readonly public: { readonly host: string; readonly port: number };
  readonly admin: { readonly host: string; readonly port: number };
  readonly logging: {
    readonly format: 'text' | 'json';
    readonly level: 'error' | 'warn' | 'info' | 'debug';
    readonly directory: string;
  };
  readonly workers: number;
  readonly shutdownTimeoutMs: number;
  readonly observability: {
    readonly health: boolean;
    readonly metrics: boolean;
  };
  readonly retention?: NonNullable<RuntimeConfigurationV1['retention']>;
  readonly backup?: { readonly directory: string };
  readonly sources: Readonly<
    Record<
      | 'deploymentName'
      | 'statePath'
      | 'public.host'
      | 'public.port'
      | 'admin.host'
      | 'admin.port'
      | 'logging.format'
      | 'logging.level'
      | 'logging.directory'
      | 'workers'
      | 'shutdownTimeoutMs',
      'cli' | 'environment' | 'config' | 'default'
    >
  >;
}

export type OpenTelemetryConfigurationSource =
  | 'environment'
  | 'config'
  | 'default';

export interface ResolvedOpenTelemetryConfigurationV1 {
  readonly profile: typeof OPEN_TELEMETRY_CONFIGURATION_PROFILE;
  readonly enabled: boolean;
  readonly protocol: 'http/protobuf';
  readonly serviceName: string;
  readonly resourceAttributes: Readonly<Record<string, string>>;
  readonly signals: {
    readonly traces: boolean;
    readonly metrics: boolean;
    readonly logs: boolean;
  };
  readonly endpoints: {
    readonly traces: string;
    readonly metrics: string;
    readonly logs: string;
  };
  readonly headerNames: {
    readonly common: readonly string[];
    readonly traces: readonly string[];
    readonly metrics: readonly string[];
    readonly logs: readonly string[];
  };
  readonly sampler:
    | { readonly name: 'always_on' | 'always_off' }
    | {
        readonly name: 'parentbased_traceidratio';
        readonly ratio: number;
      };
  readonly exportIntervalMs: number;
  readonly exportTimeoutMs: number;
  readonly shutdownTimeoutMs: number;
  readonly maxQueueSize: number;
  readonly maxExportBatchSize: number;
  readonly propagation: boolean;
  readonly sources: {
    readonly enabled: OpenTelemetryConfigurationSource;
    readonly serviceName: OpenTelemetryConfigurationSource;
    readonly resourceAttributes: OpenTelemetryConfigurationSource;
    readonly traces: OpenTelemetryConfigurationSource;
    readonly metrics: OpenTelemetryConfigurationSource;
    readonly logs: OpenTelemetryConfigurationSource;
    readonly sampler: OpenTelemetryConfigurationSource;
    readonly endpoints: {
      readonly traces: OpenTelemetryConfigurationSource;
      readonly metrics: OpenTelemetryConfigurationSource;
      readonly logs: OpenTelemetryConfigurationSource;
    };
  };
}

export interface OpenTelemetrySecretsV1 {
  readonly headers: {
    readonly common: Readonly<Record<string, string>>;
    readonly traces: Readonly<Record<string, string>>;
    readonly metrics: Readonly<Record<string, string>>;
    readonly logs: Readonly<Record<string, string>>;
  };
}

export interface ResolvedRuntimeConfigurationV2
  extends Omit<ResolvedRuntimeConfigurationV1, 'profile' | 'schemaVersion' | 'observability'> {
  readonly profile: typeof RUNTIME_CONFIG_PROFILE_V2;
  readonly schemaVersion: 2;
  readonly observability: {
    readonly health: boolean;
    readonly metrics: boolean;
    readonly openTelemetry: ResolvedOpenTelemetryConfigurationV1;
  };
}

export type ResolvedRuntimeConfiguration =
  | ResolvedRuntimeConfigurationV1
  | ResolvedRuntimeConfigurationV2;

export interface RuntimePreflightV1 {
  readonly profile: 'woml.runtime-preflight/v1';
  readonly state: {
    readonly path: string;
    readonly existing: boolean;
    readonly writableAncestor: string;
    readonly availableBytes: number;
  };
  readonly logging: {
    readonly directory: string;
    readonly writableAncestor: string;
  };
  readonly backup?: {
    readonly directory: string;
    readonly writableAncestor: string;
  };
  readonly ports: {
    readonly public: string;
    readonly admin: string;
    readonly distinct: true;
  };
}

export class RuntimeConfigurationError extends Error {
  readonly code: string;

  constructor(code: string, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'RuntimeConfigurationError';
    this.code = code;
  }
}

function object(value: unknown, subject: string): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw invalid(`${subject} must be a JSON object.`);
  }
  return value as Record<string, unknown>;
}

function only(value: Record<string, unknown>, allowed: readonly string[], subject: string): void {
  const unknown = Object.keys(value).filter(key => !allowed.includes(key));
  if (unknown.length > 0) {
    throw invalid(`${subject} contains unknown field${unknown.length === 1 ? '' : 's'}: ${unknown.join(', ')}.`);
  }
}

function invalid(message: string): RuntimeConfigurationError {
  return new RuntimeConfigurationError('WOML_RUNTIME_CONFIG_INVALID', message);
}

function optionalString(value: unknown, subject: string, max = 4096): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== 'string' || value.length === 0 || value.length > max || value.includes('\0')) {
    throw invalid(`${subject} must be a non-empty string no longer than ${max} characters.`);
  }
  return value;
}

function optionalInteger(
  value: unknown,
  subject: string,
  minimum: number,
  maximum: number
): number | undefined {
  if (value === undefined) return undefined;
  if (!Number.isSafeInteger(value) || (value as number) < minimum || (value as number) > maximum) {
    throw invalid(`${subject} must be an integer from ${minimum} through ${maximum}.`);
  }
  return value as number;
}

function optionalNumber(
  value: unknown,
  subject: string,
  minimum: number,
  maximum: number
): number | undefined {
  if (value === undefined) return undefined;
  if (
    typeof value !== 'number' ||
    !Number.isFinite(value) ||
    value < minimum ||
    value > maximum
  ) {
    throw invalid(`${subject} must be a finite number from ${minimum} through ${maximum}.`);
  }
  return value;
}

function optionalBoolean(value: unknown, subject: string): boolean | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== 'boolean') throw invalid(`${subject} must be true or false.`);
  return value;
}

function enumValue<T extends string>(
  value: unknown,
  subject: string,
  values: readonly T[]
): T | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== 'string' || !values.includes(value as T)) {
    throw invalid(`${subject} must be one of: ${values.join(', ')}.`);
  }
  return value as T;
}

function host(value: unknown, subject: string): string | undefined {
  const parsed = optionalString(value, subject, 253);
  if (parsed !== undefined && (!hostPattern.test(parsed) || /[\s/]/.test(parsed))) {
    throw invalid(`${subject} must be an IP address or hostname without a port.`);
  }
  return parsed;
}

function parseOpenTelemetryConfiguration(
  value: unknown
): OpenTelemetryConfigurationV1 {
  const telemetry = object(value, 'observability.openTelemetry');
  only(
    telemetry,
    [
      'enabled',
      'protocol',
      'traces',
      'metrics',
      'logs',
      'samplingRatio',
      'exportIntervalMs',
      'exportTimeoutMs',
      'shutdownTimeoutMs',
      'maxQueueSize',
      'maxExportBatchSize',
      'propagation',
    ],
    'observability.openTelemetry'
  );
  const maxQueueSize = optionalInteger(
    telemetry.maxQueueSize,
    'observability.openTelemetry.maxQueueSize',
    128,
    OPEN_TELEMETRY_MAX_QUEUE_SIZE
  );
  const maxExportBatchSize = optionalInteger(
    telemetry.maxExportBatchSize,
    'observability.openTelemetry.maxExportBatchSize',
    1,
    OPEN_TELEMETRY_MAX_BATCH_SIZE
  );
  if (
    maxQueueSize !== undefined &&
    maxExportBatchSize !== undefined &&
    maxExportBatchSize > maxQueueSize
  ) {
    throw invalid(
      'observability.openTelemetry.maxExportBatchSize cannot exceed maxQueueSize.'
    );
  }
  return {
    ...(telemetry.enabled === undefined
      ? {}
      : {
          enabled: optionalBoolean(
            telemetry.enabled,
            'observability.openTelemetry.enabled'
          )!,
        }),
    ...(telemetry.protocol === undefined
      ? {}
      : {
          protocol: enumValue(
            telemetry.protocol,
            'observability.openTelemetry.protocol',
            ['http/protobuf'] as const
          )!,
        }),
    ...(telemetry.traces === undefined
      ? {}
      : {
          traces: optionalBoolean(
            telemetry.traces,
            'observability.openTelemetry.traces'
          )!,
        }),
    ...(telemetry.metrics === undefined
      ? {}
      : {
          metrics: optionalBoolean(
            telemetry.metrics,
            'observability.openTelemetry.metrics'
          )!,
        }),
    ...(telemetry.logs === undefined
      ? {}
      : {
          logs: optionalBoolean(
            telemetry.logs,
            'observability.openTelemetry.logs'
          )!,
        }),
    ...(telemetry.samplingRatio === undefined
      ? {}
      : {
          samplingRatio: optionalNumber(
            telemetry.samplingRatio,
            'observability.openTelemetry.samplingRatio',
            0,
            1
          )!,
        }),
    ...(telemetry.exportIntervalMs === undefined
      ? {}
      : {
          exportIntervalMs: optionalInteger(
            telemetry.exportIntervalMs,
            'observability.openTelemetry.exportIntervalMs',
            1_000,
            300_000
          )!,
        }),
    ...(telemetry.exportTimeoutMs === undefined
      ? {}
      : {
          exportTimeoutMs: optionalInteger(
            telemetry.exportTimeoutMs,
            'observability.openTelemetry.exportTimeoutMs',
            100,
            30_000
          )!,
        }),
    ...(telemetry.shutdownTimeoutMs === undefined
      ? {}
      : {
          shutdownTimeoutMs: optionalInteger(
            telemetry.shutdownTimeoutMs,
            'observability.openTelemetry.shutdownTimeoutMs',
            100,
            30_000
          )!,
        }),
    ...(maxQueueSize === undefined ? {} : { maxQueueSize }),
    ...(maxExportBatchSize === undefined ? {} : { maxExportBatchSize }),
    ...(telemetry.propagation === undefined
      ? {}
      : {
          propagation: optionalBoolean(
            telemetry.propagation,
            'observability.openTelemetry.propagation'
          )!,
        }),
  };
}

function parseConfiguration(value: unknown): RuntimeConfiguration {
  const root = object(value, 'runtime configuration');
  only(root, [
    'schemaVersion', 'deploymentName', 'statePath', 'public', 'admin', 'logging',
    'workers', 'shutdownTimeoutMs', 'observability', 'retention', 'backup',
  ], 'runtime configuration');
  if (root.schemaVersion !== 1 && root.schemaVersion !== 2) {
    throw new RuntimeConfigurationError(
      'WOML_RUNTIME_CONFIG_UNSUPPORTED',
      'runtime configuration schemaVersion must be 1 or 2.'
    );
  }
  const deploymentName = optionalString(root.deploymentName, 'deploymentName', 128);
  if (deploymentName !== undefined && !deploymentPattern.test(deploymentName)) {
    throw invalid('deploymentName must use lowercase letters, digits, dots, underscores, or hyphens and start with a letter.');
  }

  const publicConfig = root.public === undefined ? undefined : object(root.public, 'public');
  if (publicConfig !== undefined) only(publicConfig, ['host', 'port'], 'public');
  const adminConfig = root.admin === undefined ? undefined : object(root.admin, 'admin');
  if (adminConfig !== undefined) only(adminConfig, ['host', 'port'], 'admin');
  const logging = root.logging === undefined ? undefined : object(root.logging, 'logging');
  if (logging !== undefined) only(logging, ['format', 'level', 'directory'], 'logging');
  const observability = root.observability === undefined ? undefined : object(root.observability, 'observability');
  if (observability !== undefined) {
    only(
      observability,
      root.schemaVersion === 1
        ? ['health', 'metrics']
        : ['health', 'metrics', 'openTelemetry'],
      'observability'
    );
  }
  const openTelemetry =
    observability?.openTelemetry === undefined
      ? undefined
      : parseOpenTelemetryConfiguration(observability.openTelemetry);
  const retention = root.retention === undefined ? undefined : object(root.retention, 'retention');
  if (retention !== undefined) {
    only(retention, ['enabled', 'succeededAfterDays', 'failedAfterDays', 'cancelledAfterDays', 'maintenanceHourUtc'], 'retention');
    if (typeof retention.enabled !== 'boolean') throw invalid('retention.enabled is required and must be true or false.');
  }
  const backup = root.backup === undefined ? undefined : object(root.backup, 'backup');
  if (backup !== undefined) {
    only(backup, ['directory'], 'backup');
    if (backup.directory === undefined) throw invalid('backup.directory is required when backup is configured.');
  }

  const parsed = {
    ...(deploymentName === undefined ? {} : { deploymentName }),
    ...(root.statePath === undefined ? {} : { statePath: optionalString(root.statePath, 'statePath')! }),
    ...(publicConfig === undefined ? {} : { public: {
      ...(publicConfig.host === undefined ? {} : { host: host(publicConfig.host, 'public.host')! }),
      ...(publicConfig.port === undefined ? {} : { port: optionalInteger(publicConfig.port, 'public.port', 1, 65_535)! }),
    } }),
    ...(adminConfig === undefined ? {} : { admin: {
      ...(adminConfig.host === undefined ? {} : { host: host(adminConfig.host, 'admin.host')! }),
      ...(adminConfig.port === undefined ? {} : { port: optionalInteger(adminConfig.port, 'admin.port', 1, 65_535)! }),
    } }),
    ...(logging === undefined ? {} : { logging: {
      ...(logging.format === undefined ? {} : { format: enumValue(logging.format, 'logging.format', ['text', 'json'] as const)! }),
      ...(logging.level === undefined ? {} : { level: enumValue(logging.level, 'logging.level', ['error', 'warn', 'info', 'debug'] as const)! }),
      ...(logging.directory === undefined ? {} : { directory: optionalString(logging.directory, 'logging.directory')! }),
    } }),
    ...(root.workers === undefined ? {} : { workers: optionalInteger(root.workers, 'workers', 1, 256)! }),
    ...(root.shutdownTimeoutMs === undefined ? {} : { shutdownTimeoutMs: optionalInteger(root.shutdownTimeoutMs, 'shutdownTimeoutMs', 1_000, 300_000)! }),
    ...(observability === undefined ? {} : { observability: {
      ...(observability.health === undefined ? {} : { health: optionalBoolean(observability.health, 'observability.health')! }),
      ...(observability.metrics === undefined ? {} : { metrics: optionalBoolean(observability.metrics, 'observability.metrics')! }),
      ...(openTelemetry === undefined ? {} : { openTelemetry }),
    } }),
    ...(retention === undefined ? {} : { retention: {
      enabled: retention.enabled as boolean,
      ...(retention.succeededAfterDays === undefined ? {} : { succeededAfterDays: optionalInteger(retention.succeededAfterDays, 'retention.succeededAfterDays', 1, 3_650)! }),
      ...(retention.failedAfterDays === undefined ? {} : { failedAfterDays: optionalInteger(retention.failedAfterDays, 'retention.failedAfterDays', 1, 3_650)! }),
      ...(retention.cancelledAfterDays === undefined ? {} : { cancelledAfterDays: optionalInteger(retention.cancelledAfterDays, 'retention.cancelledAfterDays', 1, 3_650)! }),
      ...(retention.maintenanceHourUtc === undefined ? {} : { maintenanceHourUtc: optionalInteger(retention.maintenanceHourUtc, 'retention.maintenanceHourUtc', 0, 23)! }),
    } }),
    ...(backup === undefined ? {} : { backup: {
      ...(backup.directory === undefined ? {} : { directory: optionalString(backup.directory, 'backup.directory')! }),
    } }),
  };
  return root.schemaVersion === 1
    ? { schemaVersion: 1, ...parsed } as RuntimeConfigurationV1
    : { schemaVersion: 2, ...parsed } as RuntimeConfigurationV2;
}

function envInteger(environment: Readonly<Record<string, string | undefined>>, name: string, minimum: number, maximum: number): number | undefined {
  const raw = environment[name];
  if (raw === undefined) return undefined;
  if (!/^\d+$/.test(raw)) throw invalid(`${name} must be an integer from ${minimum} through ${maximum}.`);
  return optionalInteger(Number(raw), name, minimum, maximum);
}

function environmentBoolean(
  environment: Readonly<Record<string, string | undefined>>,
  name: string
): boolean | undefined {
  const raw = environment[name];
  if (raw === undefined) return undefined;
  if (raw.toLowerCase() === 'true') return true;
  if (raw.toLowerCase() === 'false') return false;
  throw invalid(`${name} must be true or false.`);
}

function openTelemetryEndpoint(
  raw: string | undefined,
  subject: string
): string | undefined {
  if (raw === undefined) return undefined;
  if (raw.length === 0 || raw.length > 2_048 || raw.includes('\0')) {
    throw invalid(`${subject} must be a non-empty URL no longer than 2048 characters.`);
  }
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    throw invalid(`${subject} must be a valid absolute HTTP or HTTPS URL.`);
  }
  if (
    (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') ||
    parsed.hostname.length === 0 ||
    parsed.username.length > 0 ||
    parsed.password.length > 0 ||
    parsed.search.length > 0 ||
    parsed.hash.length > 0
  ) {
    throw invalid(
      `${subject} must be an absolute HTTP or HTTPS URL without credentials, query, or fragment.`
    );
  }
  return parsed.toString();
}

function signalEndpoint(base: string, signal: 'traces' | 'metrics' | 'logs'): string {
  const normalized = base.endsWith('/') ? base : `${base}/`;
  return new URL(`v1/${signal}`, normalized).toString();
}

function parseOpenTelemetryHeaders(
  raw: string | undefined,
  subject: string
): Readonly<Record<string, string>> {
  if (raw === undefined || raw.length === 0) return {};
  if (raw.length > 8_192 || raw.includes('\0') || /[\r\n]/u.test(raw)) {
    throw invalid(`${subject} contains an invalid or oversized header list.`);
  }
  const result: Record<string, string> = {};
  const entries = raw.split(',');
  if (entries.length > 32) {
    throw invalid(`${subject} may contain at most 32 headers.`);
  }
  for (const entry of entries) {
    const separator = entry.indexOf('=');
    if (separator <= 0) {
      throw invalid(`${subject} contains a malformed header entry.`);
    }
    const name = entry.slice(0, separator).trim().toLowerCase();
    if (!/^[a-z0-9][a-z0-9_.-]{0,127}$/u.test(name)) {
      throw invalid(`${subject} contains an invalid header name.`);
    }
    let value: string;
    try {
      value = decodeURIComponent(entry.slice(separator + 1).trim());
    } catch {
      throw invalid(`${subject} contains an invalid encoded header value.`);
    }
    if (value.length > 4_096 || value.includes('\0') || /[\r\n]/u.test(value)) {
      throw invalid(`${subject} contains an invalid or oversized header value.`);
    }
    if (Object.hasOwn(result, name)) {
      throw invalid(`${subject} contains a duplicate header name.`);
    }
    result[name] = value;
  }
  return Object.freeze(result);
}

const OPEN_TELEMETRY_RESOURCE_KEYS = new Set([
  'cloud.provider',
  'cloud.region',
  'deployment.environment.name',
  'service.namespace',
]);

function parseOpenTelemetryResourceAttributes(
  raw: string | undefined
): Readonly<Record<string, string>> {
  if (raw === undefined || raw.length === 0) return {};
  if (raw.length > 4_096 || raw.includes('\0') || /[\r\n]/u.test(raw)) {
    throw invalid('OTEL_RESOURCE_ATTRIBUTES contains invalid or oversized data.');
  }
  const result: Record<string, string> = {};
  const entries = raw.split(',');
  if (entries.length > 16) {
    throw invalid('OTEL_RESOURCE_ATTRIBUTES may contain at most 16 attributes.');
  }
  for (const entry of entries) {
    const separator = entry.indexOf('=');
    if (separator <= 0) {
      throw invalid('OTEL_RESOURCE_ATTRIBUTES contains a malformed attribute.');
    }
    const key = entry.slice(0, separator).trim();
    if (!OPEN_TELEMETRY_RESOURCE_KEYS.has(key) || Object.hasOwn(result, key)) {
      throw invalid(
        `OTEL_RESOURCE_ATTRIBUTES contains an unsupported or duplicate key: ${key || '<empty>'}.`
      );
    }
    let value: string;
    try {
      value = decodeURIComponent(entry.slice(separator + 1).trim());
    } catch {
      throw invalid('OTEL_RESOURCE_ATTRIBUTES contains an invalid encoded value.');
    }
    if (value.length === 0 || value.length > 256 || /[\u0000-\u001f\u007f]/u.test(value)) {
      throw invalid('OTEL_RESOURCE_ATTRIBUTES contains an invalid or oversized value.');
    }
    result[key] = value;
  }
  return Object.freeze(result);
}

function signalEnabledFromEnvironment(
  environment: Readonly<Record<string, string | undefined>>,
  signal: 'TRACES' | 'METRICS' | 'LOGS'
): boolean | undefined {
  const name = `OTEL_${signal}_EXPORTER`;
  const raw = environment[name];
  if (raw === undefined) return undefined;
  if (raw === 'otlp') return true;
  if (raw === 'none') return false;
  throw invalid(`${name} must be otlp or none.`);
}

function resolveOpenTelemetrySampler(
  configuration: OpenTelemetryConfigurationV1 | undefined,
  environment: Readonly<Record<string, string | undefined>>
): [ResolvedOpenTelemetryConfigurationV1['sampler'], OpenTelemetryConfigurationSource] {
  const raw = environment.OTEL_TRACES_SAMPLER;
  if (raw !== undefined) {
    if (raw === 'always_on') return [{ name: 'always_on' }, 'environment'];
    if (raw === 'always_off') return [{ name: 'always_off' }, 'environment'];
    if (raw !== 'parentbased_traceidratio') {
      throw invalid(
        'OTEL_TRACES_SAMPLER must be always_on, always_off, or parentbased_traceidratio.'
      );
    }
    const argument = environment.OTEL_TRACES_SAMPLER_ARG;
    if (argument === undefined || argument.trim().length === 0) {
      throw invalid(
        'OTEL_TRACES_SAMPLER_ARG is required for parentbased_traceidratio.'
      );
    }
    const ratio = Number(argument);
    if (!Number.isFinite(ratio) || ratio < 0 || ratio > 1) {
      throw invalid('OTEL_TRACES_SAMPLER_ARG must be a number from 0 through 1.');
    }
    return [{ name: 'parentbased_traceidratio', ratio }, 'environment'];
  }
  if (environment.OTEL_TRACES_SAMPLER_ARG !== undefined) {
    throw invalid('OTEL_TRACES_SAMPLER_ARG requires OTEL_TRACES_SAMPLER.');
  }
  if (configuration?.samplingRatio !== undefined) {
    return [
      {
        name: 'parentbased_traceidratio',
        ratio: configuration.samplingRatio,
      },
      'config',
    ];
  }
  return [{ name: 'parentbased_traceidratio', ratio: 1 }, 'default'];
}

function openTelemetryHeadersFromEnvironment(
  environment: Readonly<Record<string, string | undefined>>
): OpenTelemetrySecretsV1['headers'] {
  return {
    common: parseOpenTelemetryHeaders(
      environment.OTEL_EXPORTER_OTLP_HEADERS,
      'OTEL_EXPORTER_OTLP_HEADERS'
    ),
    traces: parseOpenTelemetryHeaders(
      environment.OTEL_EXPORTER_OTLP_TRACES_HEADERS,
      'OTEL_EXPORTER_OTLP_TRACES_HEADERS'
    ),
    metrics: parseOpenTelemetryHeaders(
      environment.OTEL_EXPORTER_OTLP_METRICS_HEADERS,
      'OTEL_EXPORTER_OTLP_METRICS_HEADERS'
    ),
    logs: parseOpenTelemetryHeaders(
      environment.OTEL_EXPORTER_OTLP_LOGS_HEADERS,
      'OTEL_EXPORTER_OTLP_LOGS_HEADERS'
    ),
  };
}

function headerNames(
  headers: Readonly<Record<string, string>>
): readonly string[] {
  return Object.keys(headers).sort();
}

function resolveOpenTelemetryConfiguration(
  configuration: OpenTelemetryConfigurationV1 | undefined,
  environment: Readonly<Record<string, string | undefined>>
): ResolvedOpenTelemetryConfigurationV1 {
  const disabled = environmentBoolean(environment, 'OTEL_SDK_DISABLED');
  const commonEndpoint = openTelemetryEndpoint(
    environment.OTEL_EXPORTER_OTLP_ENDPOINT,
    'OTEL_EXPORTER_OTLP_ENDPOINT'
  );
  const specificEndpoints = {
    traces: openTelemetryEndpoint(
      environment.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
      'OTEL_EXPORTER_OTLP_TRACES_ENDPOINT'
    ),
    metrics: openTelemetryEndpoint(
      environment.OTEL_EXPORTER_OTLP_METRICS_ENDPOINT,
      'OTEL_EXPORTER_OTLP_METRICS_ENDPOINT'
    ),
    logs: openTelemetryEndpoint(
      environment.OTEL_EXPORTER_OTLP_LOGS_ENDPOINT,
      'OTEL_EXPORTER_OTLP_LOGS_ENDPOINT'
    ),
  };
  const endpointEnables =
    commonEndpoint !== undefined ||
    Object.values(specificEndpoints).some(value => value !== undefined);
  const enabled =
    disabled === true
      ? false
      : endpointEnables
        ? true
        : configuration?.enabled ?? false;
  const enabledSource: OpenTelemetryConfigurationSource =
    disabled !== undefined || endpointEnables
      ? 'environment'
      : configuration?.enabled !== undefined
        ? 'config'
        : 'default';

  const protocolVariables = [
    'OTEL_EXPORTER_OTLP_PROTOCOL',
    'OTEL_EXPORTER_OTLP_TRACES_PROTOCOL',
    'OTEL_EXPORTER_OTLP_METRICS_PROTOCOL',
    'OTEL_EXPORTER_OTLP_LOGS_PROTOCOL',
  ] as const;
  if (enabled) {
    for (const name of protocolVariables) {
      const protocol = environment[name];
      if (protocol !== undefined && protocol !== 'http/protobuf') {
        throw invalid(`${name} must be http/protobuf.`);
      }
    }
  }

  const traceEnvironment = enabled
    ? signalEnabledFromEnvironment(environment, 'TRACES')
    : undefined;
  const metricEnvironment = enabled
    ? signalEnabledFromEnvironment(environment, 'METRICS')
    : undefined;
  const logEnvironment = enabled
    ? signalEnabledFromEnvironment(environment, 'LOGS')
    : undefined;
  const traces = traceEnvironment ?? configuration?.traces ?? true;
  const metrics = metricEnvironment ?? configuration?.metrics ?? true;
  const logs = logEnvironment ?? configuration?.logs ?? false;
  if (enabled && !traces && !metrics && !logs) {
    throw invalid('OpenTelemetry is enabled but every signal is disabled.');
  }

  const serviceName = optionalString(
    environment.OTEL_SERVICE_NAME,
    'OTEL_SERVICE_NAME',
    128
  ) ?? 'woml-runtime';
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._/-]{0,127}$/u.test(serviceName)) {
    throw invalid('OTEL_SERVICE_NAME contains unsupported characters.');
  }
  const resourceAttributes = enabled
    ? parseOpenTelemetryResourceAttributes(environment.OTEL_RESOURCE_ATTRIBUTES)
    : {};
  const [sampler, samplerSource] = enabled
    ? resolveOpenTelemetrySampler(configuration, environment)
    : [
        { name: 'parentbased_traceidratio', ratio: 1 } as const,
        'default' as const,
      ];
  const headers = enabled
    ? openTelemetryHeadersFromEnvironment(environment)
    : { common: {}, traces: {}, metrics: {}, logs: {} };
  const baseEndpoint = commonEndpoint ?? OPEN_TELEMETRY_DEFAULT_ENDPOINT;
  const endpoints = {
    traces:
      specificEndpoints.traces ?? signalEndpoint(baseEndpoint, 'traces'),
    metrics:
      specificEndpoints.metrics ?? signalEndpoint(baseEndpoint, 'metrics'),
    logs: specificEndpoints.logs ?? signalEndpoint(baseEndpoint, 'logs'),
  };
  const maxQueueSize =
    configuration?.maxQueueSize ?? OPEN_TELEMETRY_DEFAULT_QUEUE_SIZE;
  const maxExportBatchSize =
    configuration?.maxExportBatchSize ?? OPEN_TELEMETRY_DEFAULT_BATCH_SIZE;
  if (maxExportBatchSize > maxQueueSize) {
    throw invalid(
      'observability.openTelemetry.maxExportBatchSize cannot exceed maxQueueSize.'
    );
  }
  const exportIntervalMs =
    configuration?.exportIntervalMs ??
    OPEN_TELEMETRY_DEFAULT_EXPORT_INTERVAL_MS;
  const exportTimeoutMs =
    configuration?.exportTimeoutMs ??
    OPEN_TELEMETRY_DEFAULT_EXPORT_TIMEOUT_MS;
  if (enabled && metrics && exportTimeoutMs >= exportIntervalMs) {
    throw invalid(
      'observability.openTelemetry.exportTimeoutMs must be less than exportIntervalMs when metrics export is enabled.'
    );
  }
  const endpointSource = (
    specific: string | undefined
  ): OpenTelemetryConfigurationSource =>
    specific !== undefined || commonEndpoint !== undefined
      ? 'environment'
      : 'default';
  return {
    profile: OPEN_TELEMETRY_CONFIGURATION_PROFILE,
    enabled,
    protocol: 'http/protobuf',
    serviceName,
    resourceAttributes,
    signals: { traces, metrics, logs },
    endpoints,
    headerNames: {
      common: headerNames(headers.common),
      traces: headerNames(headers.traces),
      metrics: headerNames(headers.metrics),
      logs: headerNames(headers.logs),
    },
    sampler,
    exportIntervalMs,
    exportTimeoutMs,
    shutdownTimeoutMs:
      configuration?.shutdownTimeoutMs ??
      OPEN_TELEMETRY_DEFAULT_SHUTDOWN_TIMEOUT_MS,
    maxQueueSize,
    maxExportBatchSize,
    propagation: configuration?.propagation ?? true,
    sources: {
      enabled: enabledSource,
      serviceName:
        environment.OTEL_SERVICE_NAME === undefined ? 'default' : 'environment',
      resourceAttributes:
        environment.OTEL_RESOURCE_ATTRIBUTES === undefined
          ? 'default'
          : 'environment',
      traces:
        traceEnvironment !== undefined
          ? 'environment'
          : configuration?.traces !== undefined
            ? 'config'
            : 'default',
      metrics:
        metricEnvironment !== undefined
          ? 'environment'
          : configuration?.metrics !== undefined
            ? 'config'
            : 'default',
      logs:
        logEnvironment !== undefined
          ? 'environment'
          : configuration?.logs !== undefined
            ? 'config'
            : 'default',
      sampler: samplerSource,
      endpoints: {
        traces: endpointSource(specificEndpoints.traces),
        metrics: endpointSource(specificEndpoints.metrics),
        logs: endpointSource(specificEndpoints.logs),
      },
    },
  };
}

export function resolveOpenTelemetrySecrets(
  configuration: ResolvedOpenTelemetryConfigurationV1,
  environment: Readonly<Record<string, string | undefined>> = process.env
): OpenTelemetrySecretsV1 {
  if (!configuration.enabled) {
    return { headers: { common: {}, traces: {}, metrics: {}, logs: {} } };
  }
  const headers = openTelemetryHeadersFromEnvironment(environment);
  for (const signal of ['common', 'traces', 'metrics', 'logs'] as const) {
    const names = headerNames(headers[signal]);
    if (names.join('\0') !== configuration.headerNames[signal].join('\0')) {
      throw new RuntimeConfigurationError(
        'WOML_OTEL_CONFIGURATION_CHANGED',
        'OpenTelemetry exporter headers changed after runtime configuration was resolved.'
      );
    }
  }
  return { headers };
}

function value<T>(cli: T | undefined, environment: T | undefined, config: T | undefined, fallback: T): [T, 'cli' | 'environment' | 'config' | 'default'] {
  if (cli !== undefined) return [cli, 'cli'];
  if (environment !== undefined) return [environment, 'environment'];
  if (config !== undefined) return [config, 'config'];
  return [fallback, 'default'];
}

export async function resolveRuntimeConfiguration(
  rawConfigPath?: string,
  overrides: RuntimeConfigurationOverrides = {},
  environment: Readonly<Record<string, string | undefined>> = process.env,
  cwd = process.cwd()
): Promise<ResolvedRuntimeConfiguration> {
  let config: RuntimeConfiguration | undefined;
  let configPath: string | undefined;
  let base = cwd;
  if (rawConfigPath !== undefined) {
    configPath = resolve(cwd, rawConfigPath);
    let file;
    try {
      file = await stat(configPath);
    } catch (error) {
      throw new RuntimeConfigurationError('WOML_RUNTIME_CONFIG_INVALID', `runtime configuration file does not exist: ${configPath}`, { cause: error });
    }
    if (!file.isFile() || file.size > RUNTIME_CONFIG_MAX_BYTES) {
      throw invalid(`runtime configuration must be a file no larger than ${RUNTIME_CONFIG_MAX_BYTES} bytes.`);
    }
    let decoded: unknown;
    try {
      decoded = JSON.parse(await Bun.file(configPath).text());
    } catch (error) {
      throw new RuntimeConfigurationError('WOML_RUNTIME_CONFIG_INVALID', 'runtime configuration must contain valid JSON.', { cause: error });
    }
    config = parseConfiguration(decoded);
    base = dirname(configPath);
  }

  const envDeployment = optionalString(environment.WOML_RUNTIME_DEPLOYMENT, 'WOML_RUNTIME_DEPLOYMENT', 128);
  if (envDeployment !== undefined && !deploymentPattern.test(envDeployment)) throw invalid('WOML_RUNTIME_DEPLOYMENT has an invalid deployment name.');
  const cliDeployment = optionalString(overrides.deploymentName, 'deploymentName', 128);
  if (cliDeployment !== undefined && !deploymentPattern.test(cliDeployment)) throw invalid('deploymentName has an invalid deployment name.');
  const cliStatePath = optionalString(overrides.statePath, 'statePath');
  const envStatePath = optionalString(environment.WOML_RUNTIME_STATE, 'WOML_RUNTIME_STATE');
  const cliPublicHost = host(overrides.publicHost, 'public.host');
  const envPublicHost = host(environment.WOML_RUNTIME_HOST, 'WOML_RUNTIME_HOST');
  const cliAdminHost = host(overrides.adminHost, 'admin.host');
  const envAdminHost = host(environment.WOML_RUNTIME_ADMIN_HOST, 'WOML_RUNTIME_ADMIN_HOST');
  const cliLogFormat = enumValue(overrides.logFormat, 'logging.format', ['text', 'json'] as const);
  const envLogFormat = enumValue(environment.WOML_RUNTIME_LOG_FORMAT, 'WOML_RUNTIME_LOG_FORMAT', ['text', 'json'] as const);
  const cliLogLevel = enumValue(overrides.logLevel, 'logging.level', ['error', 'warn', 'info', 'debug'] as const);
  const envLogLevel = enumValue(environment.WOML_RUNTIME_LOG_LEVEL, 'WOML_RUNTIME_LOG_LEVEL', ['error', 'warn', 'info', 'debug'] as const);
  const cliLogDirectory = optionalString(overrides.logDirectory, 'logging.directory');
  const envLogDirectory = optionalString(environment.WOML_RUNTIME_LOG_DIRECTORY, 'WOML_RUNTIME_LOG_DIRECTORY');
  const [deploymentName, deploymentSource] = value(cliDeployment, envDeployment, config?.deploymentName, 'default');
  const [rawStatePath, stateSource] = value(cliStatePath, envStatePath, config?.statePath, '.woml/state.sqlite');
  const [publicHost, publicHostSource] = value(cliPublicHost, envPublicHost, config?.public?.host, '127.0.0.1');
  const [publicPort, publicPortSource] = value(optionalInteger(overrides.publicPort, 'public.port', 1, 65_535), envInteger(environment, 'WOML_RUNTIME_PORT', 1, 65_535), config?.public?.port, 3_000);
  const [adminHost, adminHostSource] = value(cliAdminHost, envAdminHost, config?.admin?.host, '127.0.0.1');
  const [adminPort, adminPortSource] = value(optionalInteger(overrides.adminPort, 'admin.port', 1, 65_535), envInteger(environment, 'WOML_RUNTIME_ADMIN_PORT', 1, 65_535), config?.admin?.port, 3_001);
  const [logFormat, logFormatSource] = value(cliLogFormat, envLogFormat, config?.logging?.format, 'text' as const);
  const [logLevel, logLevelSource] = value(cliLogLevel, envLogLevel, config?.logging?.level, 'info' as const);
  const [rawLogDirectory, logDirectorySource] = value(cliLogDirectory, envLogDirectory, config?.logging?.directory, '.woml/logs');
  const [workers, workersSource] = value(optionalInteger(overrides.workers, 'workers', 1, 256), envInteger(environment, 'WOML_RUNTIME_WORKERS', 1, 256), config?.workers, 4);
  const [shutdownTimeoutMs, shutdownSource] = value(optionalInteger(overrides.shutdownTimeoutMs, 'shutdownTimeoutMs', 1_000, 300_000), envInteger(environment, 'WOML_RUNTIME_SHUTDOWN_TIMEOUT_MS', 1_000, 300_000), config?.shutdownTimeoutMs, 30_000);
  if (!['127.0.0.1', 'localhost', '::1'].includes(adminHost)) {
    throw invalid('admin.host must be a loopback address in Runtime Admin v1.');
  }
  if (publicHost === adminHost && publicPort === adminPort) {
    throw invalid('public and admin listeners cannot use the same host and port.');
  }
  const stateBase = stateSource === 'config' ? base : cwd;
  const logBase = logDirectorySource === 'config' ? base : cwd;
  const backupDirectory = config?.backup?.directory;
  const openTelemetry = resolveOpenTelemetryConfiguration(
    config?.schemaVersion === 2
      ? config.observability?.openTelemetry
      : undefined,
    environment
  );
  const resolved = {
    ...(configPath === undefined ? {} : { configPath }),
    deploymentName,
    statePath: resolve(stateBase, rawStatePath),
    public: { host: publicHost, port: publicPort },
    admin: { host: adminHost, port: adminPort },
    logging: { format: logFormat, level: logLevel, directory: resolve(logBase, rawLogDirectory) },
    workers,
    shutdownTimeoutMs,
    observability: { health: config?.observability?.health ?? true, metrics: config?.observability?.metrics ?? true },
    ...(config?.retention === undefined ? {} : { retention: config.retention }),
    ...(backupDirectory === undefined ? {} : { backup: { directory: resolve(base, backupDirectory) } }),
    sources: {
      deploymentName: deploymentSource,
      statePath: stateSource,
      'public.host': publicHostSource,
      'public.port': publicPortSource,
      'admin.host': adminHostSource,
      'admin.port': adminPortSource,
      'logging.format': logFormatSource,
      'logging.level': logLevelSource,
      'logging.directory': logDirectorySource,
      workers: workersSource,
      shutdownTimeoutMs: shutdownSource,
    },
  };
  if (config?.schemaVersion === 2 || openTelemetry.enabled) {
    return {
      profile: RUNTIME_CONFIG_PROFILE_V2,
      schemaVersion: 2,
      ...resolved,
      observability: {
        health: config?.observability?.health ?? true,
        metrics: config?.observability?.metrics ?? true,
        openTelemetry,
      },
    };
  }
  return {
    profile: RUNTIME_CONFIG_PROFILE_V1,
    schemaVersion: 1,
    ...resolved,
    observability: {
      health: config?.observability?.health ?? true,
      metrics: config?.observability?.metrics ?? true,
    },
  };
}

async function writableAncestor(
  target: string,
  accessPath: (path: string, mode?: number) => Promise<void> = access
): Promise<{ path: string; existing: boolean }> {
  let candidate = target;
  let existing = true;
  while (true) {
    try {
      const entry = await stat(candidate);
      const directory = entry.isDirectory() ? candidate : dirname(candidate);
      await accessPath(directory, constants.R_OK | constants.W_OK);
      return { path: directory, existing };
    } catch (error) {
      if (error instanceof Error && 'code' in error && error.code === 'ENOENT') {
        existing = false;
        const parent = dirname(candidate);
        if (parent === candidate) throw error;
        candidate = parent;
        continue;
      }
      throw new RuntimeConfigurationError('WOML_DEPLOYMENT_PREFLIGHT_FAILED', `path is not readable and writable: ${target}`, { cause: error });
    }
  }
}

export async function preflightRuntimeConfiguration(
  configuration: ResolvedRuntimeConfiguration,
  dependencies: {
    readonly accessPath?: (path: string, mode?: number) => Promise<void>;
    readonly statFilesystem?: (
      path: string
    ) => Promise<{ readonly bavail: number | bigint; readonly bsize: number | bigint }>;
  } = {}
): Promise<RuntimePreflightV1> {
  const state = await writableAncestor(configuration.statePath, dependencies.accessPath);
  const logs = await writableAncestor(configuration.logging.directory, dependencies.accessPath);
  const backup = configuration.backup === undefined ? undefined : await writableAncestor(configuration.backup.directory, dependencies.accessPath);
  const filesystem = await (dependencies.statFilesystem ?? statfs)(state.path);
  const availableBytes = Number(filesystem.bavail) * Number(filesystem.bsize);
  if (!Number.isSafeInteger(availableBytes) || availableBytes < RUNTIME_MIN_FREE_BYTES) {
    throw new RuntimeConfigurationError(
      'WOML_DEPLOYMENT_PREFLIGHT_FAILED',
      `state filesystem must have at least ${RUNTIME_MIN_FREE_BYTES} available bytes.`
    );
  }
  return {
    profile: 'woml.runtime-preflight/v1',
    state: { path: configuration.statePath, existing: state.existing, writableAncestor: state.path, availableBytes },
    logging: { directory: configuration.logging.directory, writableAncestor: logs.path },
    ...(configuration.backup === undefined || backup === undefined ? {} : { backup: { directory: configuration.backup.directory, writableAncestor: backup.path } }),
    ports: {
      public: `${configuration.public.host}:${configuration.public.port}`,
      admin: `${configuration.admin.host}:${configuration.admin.port}`,
      distinct: true,
    },
  };
}
