import { describe, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import Ajv2020 from 'ajv/dist/2020';
import addFormats from 'ajv-formats';

import {
  resolveOpenTelemetrySecrets,
  resolveRuntimeConfiguration,
} from '../src/runtime-config';
import { OpenTelemetryRuntime } from '../src/opentelemetry-runtime';
import { governOpenTelemetryExporter } from '../src/opentelemetry-sdk';

const repositoryRoot = resolve(import.meta.dir, '../..');
const schemaRoot = resolve(repositoryRoot, 'docs/schemas');
const fixtureRoot = resolve(import.meta.dir, 'fixtures/opentelemetry');

async function validators() {
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  addFormats(ajv);
  for (const name of [
    'runtime-configuration.v2.schema.json',
    'opentelemetry-configuration.v1.schema.json',
    'opentelemetry-exporter-health.v1.schema.json',
    'opentelemetry-exporter-health.v2.schema.json',
    'runtime-health.v2.schema.json',
    'runtime-metrics.v3.schema.json',
    'telemetry-signal.v1.schema.json',
    'telemetry-admission-link.v1.schema.json',
    'production-preflight.v2.schema.json',
  ]) {
    ajv.addSchema(await Bun.file(resolve(schemaRoot, name)).json());
  }
  return ajv;
}

async function withConfiguration(
  value: unknown,
  action: (path: string, directory: string) => Promise<void>
): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), 'woml-otel-'));
  try {
    const path = join(directory, 'woml.runtime.json');
    await Bun.write(path, JSON.stringify(value));
    await action(path, directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

describe('OpenTelemetry Export v1 foundation', () => {
  test('validates the frozen configuration, signal, and health fixtures', async () => {
    const ajv = await validators();
    const runtimeConfig = await Bun.file(
      resolve(fixtureRoot, 'runtime-config.v2.json')
    ).json();
    const contracts = await Bun.file(
      resolve(fixtureRoot, 'contracts.v1.json')
    ).json();
    const preflight = await Bun.file(
      resolve(fixtureRoot, 'preflight.v2.json')
    ).json();
    const values = [
      ['https://woml.dev/schemas/runtime-configuration/v2', runtimeConfig],
      [
        'https://woml.dev/schemas/opentelemetry-configuration/v1',
        contracts.resolvedConfiguration,
      ],
      [
        'https://woml.dev/schemas/opentelemetry-exporter-health/v1',
        contracts.health,
      ],
      ['https://woml.dev/schemas/telemetry-signal/v1', contracts.signal],
      ['https://woml.dev/schemas/production-preflight/v2', preflight],
    ] as const;
    for (const [id, value] of values) {
      const validate = ajv.getSchema(id)!;
      expect(validate(value), `${id}: ${JSON.stringify(validate.errors)}`).toBe(
        true
      );
    }

    const leaked = structuredClone(contracts.signal);
    leaked.payload = { token: 'forbidden' };
    expect(
      ajv.getSchema('https://woml.dev/schemas/telemetry-signal/v1')!(leaked)
    ).toBe(false);
    const future = { ...runtimeConfig, schemaVersion: 3 };
    expect(
      ajv.getSchema('https://woml.dev/schemas/runtime-configuration/v2')!(future)
    ).toBe(false);
  });

  test('keeps Runtime Configuration v1 frozen and resolves v2 explicitly', async () => {
    await withConfiguration({ schemaVersion: 1 }, async (path, directory) => {
      const resolved = await resolveRuntimeConfiguration(path, {}, {}, directory);
      expect(resolved.profile).toBe('woml.runtime-configuration/v1');
      expect(resolved.schemaVersion).toBe(1);
      expect(resolved.observability).toEqual({ health: true, metrics: true });
    });

    await withConfiguration(
      {
        schemaVersion: 1,
        observability: { openTelemetry: { enabled: true } },
      },
      async path => {
        await expect(resolveRuntimeConfiguration(path, {}, {})).rejects.toThrow(
          'unknown field'
        );
      }
    );

    await withConfiguration(
      {
        schemaVersion: 2,
        observability: {
          openTelemetry: {
            enabled: true,
            traces: true,
            metrics: false,
            maxQueueSize: 256,
            maxExportBatchSize: 64,
          },
        },
      },
      async (path, directory) => {
        const resolved = await resolveRuntimeConfiguration(path, {}, {}, directory);
        expect(resolved.profile).toBe('woml.runtime-configuration/v2');
        expect(resolved.schemaVersion).toBe(2);
        if (resolved.schemaVersion !== 2) throw new Error('expected v2');
        expect(resolved.observability.openTelemetry).toMatchObject({
          enabled: true,
          protocol: 'http/protobuf',
          signals: { traces: true, metrics: false, logs: false },
          maxQueueSize: 256,
          maxExportBatchSize: 64,
        });
      }
    );
  });

  test('applies reviewed OTEL environment precedence without serializing secrets', async () => {
    const secret = 'Bearer never-serialize-this';
    await withConfiguration(
      {
        schemaVersion: 2,
        observability: {
          openTelemetry: { enabled: false, samplingRatio: 0.25 },
        },
      },
      async (path, directory) => {
        const environment = {
          OTEL_EXPORTER_OTLP_ENDPOINT: 'https://collector.example.test',
          OTEL_EXPORTER_OTLP_HEADERS: `authorization=${encodeURIComponent(secret)}`,
          OTEL_SERVICE_NAME: 'checkout-runtime',
          OTEL_RESOURCE_ATTRIBUTES:
            'deployment.environment.name=production,service.namespace=commerce',
          OTEL_TRACES_SAMPLER: 'always_on',
          OTEL_LOGS_EXPORTER: 'none',
        };
        const resolved = await resolveRuntimeConfiguration(
          path,
          {},
          environment,
          directory
        );
        expect(resolved.schemaVersion).toBe(2);
        if (resolved.schemaVersion !== 2) throw new Error('expected v2');
        const telemetry = resolved.observability.openTelemetry;
        expect(telemetry).toMatchObject({
          enabled: true,
          serviceName: 'checkout-runtime',
          sampler: { name: 'always_on' },
          endpoints: {
            traces: 'https://collector.example.test/v1/traces',
            metrics: 'https://collector.example.test/v1/metrics',
            logs: 'https://collector.example.test/v1/logs',
          },
          headerNames: { common: ['authorization'] },
        });
        expect(JSON.stringify(resolved)).not.toContain(secret);
        expect(resolveOpenTelemetrySecrets(telemetry, environment)).toEqual({
          headers: {
            common: { authorization: secret },
            traces: {},
            metrics: {},
            logs: {},
          },
        });
      }
    );
  });

  test('rejects unsafe endpoints, headers, bounds, and empty signal sets', async () => {
    const cases = [
      [{ schemaVersion: 3 }, {}, 'schemaVersion must be 1 or 2'],
      [
        { schemaVersion: 2, observability: { openTelemetry: { enabled: true } } },
        { OTEL_EXPORTER_OTLP_ENDPOINT: 'file:///tmp/collector' },
        'absolute HTTP or HTTPS',
      ],
      [
        { schemaVersion: 2, observability: { openTelemetry: { enabled: true } } },
        { OTEL_EXPORTER_OTLP_HEADERS: 'authorization=ok%0D%0AX-Evil%3Ayes' },
        'invalid or oversized header value',
      ],
      [
        {
          schemaVersion: 2,
          observability: {
            openTelemetry: {
              enabled: true,
              traces: false,
              metrics: false,
              logs: false,
            },
          },
        },
        {},
        'every signal is disabled',
      ],
      [
        {
          schemaVersion: 2,
          observability: {
            openTelemetry: {
              enabled: true,
              exportIntervalMs: 1000,
              exportTimeoutMs: 1000,
            },
          },
        },
        {},
        'exportTimeoutMs must be less than exportIntervalMs',
      ],
    ] as const;
    for (const [configuration, environment, message] of cases) {
      await withConfiguration(configuration, async path => {
        await expect(
          resolveRuntimeConfiguration(path, {}, environment)
        ).rejects.toThrow(message);
      });
    }
  });

  test('does not load the SDK in disabled mode and degrades exporter failures safely', async () => {
    await withConfiguration({ schemaVersion: 2 }, async path => {
      const resolved = await resolveRuntimeConfiguration(path, {}, {});
      if (resolved.schemaVersion !== 2) throw new Error('expected v2');
      let loads = 0;
      const health = [] as string[];
      const runtime = await OpenTelemetryRuntime.start(
        resolved.observability.openTelemetry,
        {
          runtimeInstanceId: 'runtime_01',
          deploymentId: 'deployment_01',
          runtimeVersion: '1.0.10',
          environment: {},
          loadSdk: async () => {
            loads += 1;
            throw new Error('must not load');
          },
          onHealth: value => health.push(value.state),
        }
      );
      expect(loads).toBe(0);
      expect(health).toEqual(['disabled']);
      await runtime.shutdown();
      expect(health).toEqual(['disabled', 'stopped']);
    });

    await withConfiguration(
      {
        schemaVersion: 2,
        observability: {
          openTelemetry: { enabled: true, metrics: false },
        },
      },
      async path => {
        const resolved = await resolveRuntimeConfiguration(path, {}, {});
        if (resolved.schemaVersion !== 2) throw new Error('expected v2');
        const health = [] as string[];
        const runtime = await OpenTelemetryRuntime.start(
          resolved.observability.openTelemetry,
          {
            runtimeInstanceId: 'runtime_01',
            deploymentId: 'deployment_01',
            runtimeVersion: '1.0.10',
            environment: {},
            loadSdk: async () => {
              throw new Error('collector unavailable');
            },
            onHealth: value => health.push(value.state),
          }
        );
        expect(health).toEqual(['starting', 'degraded']);
        expect(runtime.health()).toMatchObject({
          state: 'degraded',
          lastErrorCode: 'WOML_OTEL_EXPORTER_UNAVAILABLE',
        });
        await runtime.shutdown();
      }
    );
  });

  test('owns enabled exporter startup, force-flush, and shutdown ordering', async () => {
    await withConfiguration(
      {
        schemaVersion: 2,
        observability: {
          openTelemetry: { enabled: true, metrics: false },
        },
      },
      async path => {
        const resolved = await resolveRuntimeConfiguration(path, {}, {});
        if (resolved.schemaVersion !== 2) throw new Error('expected v2');
        const events = [] as string[];
        const runtime = await OpenTelemetryRuntime.start(
          resolved.observability.openTelemetry,
          {
            runtimeInstanceId: 'runtime_01',
            deploymentId: 'deployment_01',
            runtimeVersion: '1.0.10',
            environment: {},
            loadSdk: async () => {
              events.push('sdk-started');
              return {
                forceFlush: async () => {
                  events.push('force-flushed');
                },
                shutdown: async () => {
                  events.push('sdk-stopped');
                },
              };
            },
            onHealth: value => events.push(`health:${value.state}`),
          }
        );
        expect(runtime.health().state).toBe('healthy');
        await runtime.shutdown();
        expect(events).toEqual([
          'health:starting',
          'sdk-started',
          'health:healthy',
          'health:flushing',
          'force-flushed',
          'sdk-stopped',
          'health:stopped',
        ]);
      }
    );
  });

  test('bounds the entire exporter flush and shutdown window', async () => {
    await withConfiguration(
      {
        schemaVersion: 2,
        observability: {
          openTelemetry: {
            enabled: true,
            metrics: false,
            shutdownTimeoutMs: 100,
          },
        },
      },
      async path => {
        const resolved = await resolveRuntimeConfiguration(path, {}, {});
        if (resolved.schemaVersion !== 2) throw new Error('expected v2');
        const runtime = await OpenTelemetryRuntime.start(
          resolved.observability.openTelemetry,
          {
            runtimeInstanceId: 'runtime_01',
            deploymentId: 'deployment_01',
            runtimeVersion: '1.0.10',
            environment: {},
            loadSdk: async () => ({
              forceFlush: async () => await new Promise<void>(() => {}),
              shutdown: async () => {},
            }),
          }
        );
        const started = performance.now();
        await runtime.shutdown();
        expect(performance.now() - started).toBeLessThan(500);
        expect(runtime.health()).toMatchObject({
          state: 'stopped',
          lastErrorCode: 'WOML_OTEL_FLUSH_TIMEOUT',
        });
      }
    );
  });

  test('buffers callbacks without blocking and publishes bounded exporter health', async () => {
    const contracts = await Bun.file(
      resolve(fixtureRoot, 'contracts.v1.json')
    ).json();
    const diagnostics: string[] = [];
    const accepted: string[] = [];
    let sdkOptions:
      | Parameters<NonNullable<Parameters<typeof OpenTelemetryRuntime.start>[1]['loadSdk']>>[0]
      | undefined;
    const runtime = await OpenTelemetryRuntime.start(
      {
        ...contracts.resolvedConfiguration,
        maxQueueSize: 128,
        maxExportBatchSize: 16,
      },
      {
        runtimeInstanceId: 'runtime_01',
        deploymentId: 'deployment_01',
        runtimeVersion: '1.0.10',
        environment: {
          OTEL_EXPORTER_OTLP_HEADERS: 'authorization=Bearer%20test',
        },
        loadSdk: async options => {
          sdkOptions = options;
          return {
            emit: signal => {
              accepted.push(signal.signalId);
              return true;
            },
            forceFlush: async () => {},
            shutdown: async () => {},
          };
        },
        onDiagnostic: code => diagnostics.push(code),
      }
    );
    for (let index = 0; index < 128; index += 1) {
      expect(runtime.record({
        ...contracts.signal,
        signalId: `telemetry_buffer_${index}`,
      })).toBe(true);
    }
    expect(runtime.record({
      ...contracts.signal,
      signalId: 'telemetry_buffer_overflow',
    })).toBe(false);
    expect(runtime.health()).toMatchObject({
      profile: 'woml.opentelemetry-exporter-health/v2',
      queue: { size: 128, capacity: 128, utilization: 1 },
      dropped: 1,
    });
    for (let index = 0; index < 9; index += 1) await Promise.resolve();
    expect(accepted).toHaveLength(128);
    expect(runtime.health().queue.size).toBe(0);

    const failedAt = '2026-10-23T12:00:00.000Z';
    sdkOptions!.onExportEvent?.({
      kind: 'export_failed',
      signal: 'traces',
      occurredAt: failedAt,
      code: 'WOML_OTEL_EXPORT_FAILED',
    });
    sdkOptions!.onExportEvent?.({
      kind: 'export_failed',
      signal: 'traces',
      occurredAt: failedAt,
      code: 'WOML_OTEL_EXPORT_FAILED',
    });
    expect(diagnostics.filter(code => code === 'WOML_OTEL_EXPORT_FAILED')).toHaveLength(1);
    expect(runtime.health()).toMatchObject({
      state: 'degraded',
      failures: 2,
      consecutiveFailures: 2,
      lastFailureAt: failedAt,
    });
    sdkOptions!.onExportEvent?.({
      kind: 'export_succeeded',
      signal: 'traces',
      occurredAt: '2026-10-23T12:00:01.000Z',
    });
    expect(runtime.health()).toMatchObject({
      state: 'healthy',
      exports: 1,
      consecutiveFailures: 0,
      circuit: 'closed',
    });
    const ajv = await validators();
    const health = runtime.health();
    const validate = ajv.getSchema(
      'https://woml.dev/schemas/opentelemetry-exporter-health/v2'
    )!;
    expect(validate(health), JSON.stringify(validate.errors)).toBe(true);
    await runtime.shutdown();
  });

  test('bounds retry, timeout, circuit breaking, and recovery without remote error text', async () => {
    let current = Date.parse('2026-10-23T12:00:00.000Z');
    let calls = 0;
    let fail = true;
    const events: { kind: string; code?: string }[] = [];
    const delegate = {
      export: (_value: unknown, callback: (result: { code: number; error?: Error }) => void) => {
        calls += 1;
        callback(
          fail
            ? { code: 1, error: new Error('collector body SECRET must not escape') }
            : { code: 0 }
        );
      },
      forceFlush: async () => {},
      shutdown: async () => {},
    };
    const exporter = governOpenTelemetryExporter(delegate, {
      signal: 'traces',
      timeoutMs: 50,
      now: () => current,
      onEvent: event => events.push(event),
    });
    const exportOnce = () =>
      new Promise<{ code: number }>(resolve => exporter.export({}, resolve));
    expect((await exportOnce()).code).toBe(1);
    expect((await exportOnce()).code).toBe(1);
    expect((await exportOnce()).code).toBe(1);
    expect(events.some(event => event.kind === 'circuit_opened')).toBe(true);
    expect((await exportOnce()).code).toBe(1);
    expect(calls).toBe(3);
    expect(events.at(-1)).toMatchObject({
      kind: 'export_dropped',
      code: 'WOML_OTEL_CIRCUIT_OPEN',
    });
    expect(JSON.stringify(events)).not.toContain('SECRET');

    current += 5_001;
    fail = false;
    expect((await exportOnce()).code).toBe(0);
    expect(events.some(event => event.kind === 'circuit_half_open')).toBe(true);
    expect(events.some(event => event.kind === 'circuit_closed')).toBe(true);

    const slowEvents: string[] = [];
    const slow = governOpenTelemetryExporter(
      {
        export: (_value: unknown, _callback: (result: { code: number }) => void) => {},
        forceFlush: async () => {},
        shutdown: async () => {},
      },
      {
        signal: 'logs',
        timeoutMs: 10,
        onEvent: event => slowEvents.push(event.kind),
      }
    );
    const timedOut = await new Promise<{ code: number }>(resolve =>
      slow.export({}, resolve)
    );
    expect(timedOut.code).toBe(1);
    expect(slowEvents).toContain('export_timeout');
  });

  test('ships schema-valid vendor-neutral deployment assets without credentials', async () => {
    const exampleRoot = resolve(repositoryRoot, 'examples/openTelemetry');
    const configurationPath = resolve(exampleRoot, 'woml.runtime.json');
    const rawConfiguration = await Bun.file(configurationPath).json();
    const ajv = await validators();
    const validate = ajv.getSchema(
      'https://woml.dev/schemas/runtime-configuration/v2'
    )!;
    expect(
      validate(rawConfiguration),
      JSON.stringify(validate.errors)
    ).toBe(true);

    const resolved = await resolveRuntimeConfiguration(
      configurationPath,
      {},
      { OTEL_EXPORTER_OTLP_ENDPOINT: 'http://127.0.0.1:4318' },
      repositoryRoot
    );
    expect(resolved.schemaVersion).toBe(2);
    if (resolved.schemaVersion !== 2) throw new Error('expected v2');
    expect(resolved.observability.openTelemetry).toMatchObject({
      enabled: true,
      protocol: 'http/protobuf',
      signals: { traces: true, metrics: true, logs: true },
      endpoints: {
        traces: 'http://127.0.0.1:4318/v1/traces',
        metrics: 'http://127.0.0.1:4318/v1/metrics',
        logs: 'http://127.0.0.1:4318/v1/logs',
      },
    });

    const compose = await Bun.file(resolve(exampleRoot, 'docker-compose.yml')).text();
    const collector = await Bun.file(
      resolve(exampleRoot, 'collector-config.yaml')
    ).text();
    expect(compose).toContain('otel/opentelemetry-collector-contrib:0.163.0');
    expect(compose).toContain('jaegertracing/jaeger:2.22.0');
    expect(compose).toContain('prom/prometheus:v3.15.1');
    expect(collector).toContain('endpoint: 0.0.0.0:4318');
    expect(collector).toContain('processors: [memory_limiter, batch]');
    for (const source of [compose, collector, JSON.stringify(rawConfiguration)]) {
      expect(source).not.toMatch(/(?:authorization|api[_-]?key|password|token)\s*[:=]/i);
    }
  });

  test('pins only the reviewed Apache-2.0 OpenTelemetry SDK line', async () => {
    const manifest = await Bun.file(resolve(import.meta.dir, '../package.json')).json();
    const expected = {
      '@opentelemetry/api': '1.10.0',
      '@opentelemetry/exporter-logs-otlp-proto': '0.300.0',
      '@opentelemetry/exporter-metrics-otlp-proto': '0.300.0',
      '@opentelemetry/exporter-trace-otlp-proto': '0.300.0',
      '@opentelemetry/resources': '3.0.0',
      '@opentelemetry/sdk-logs': '3.0.0',
      '@opentelemetry/sdk-metrics': '3.0.0',
      '@opentelemetry/sdk-trace': '3.0.0',
    } as const;
    for (const [name, version] of Object.entries(expected)) {
      expect(manifest.devDependencies[name]).toBe(version);
      const installed = await Bun.file(
        resolve(import.meta.dir, `../node_modules/${name}/package.json`)
      ).json();
      expect(installed.version).toBe(version);
      expect(installed.license).toBe('Apache-2.0');
    }
  });

  test('freezes deterministic cross-language trace and span identities', async () => {
    const contracts = await Bun.file(
      resolve(fixtureRoot, 'contracts.v1.json')
    ).json();
    const digest = (value: string) =>
      createHash('sha256').update(value).digest('hex');
    expect(digest('woml:trace:v1\0run_01HXTRACE').slice(0, 32)).toBe(
      contracts.traceIdentities.traceId
    );
    expect(
      digest('woml:span:v1\0run_01HXTRACE\0run\0run_01HXTRACE').slice(0, 16)
    ).toBe(contracts.traceIdentities.runSpanId);
    expect(
      digest(
        'woml:span:v1\0run_01HXTRACE\0attempt\0prepare:1:inv_01'
      ).slice(0, 16)
    ).toBe(contracts.traceIdentities.attemptSpanId);
  });
});
