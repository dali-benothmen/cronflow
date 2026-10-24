import { afterEach, describe, expect, test } from 'bun:test';

import type { ResolvedOpenTelemetryConfigurationV1 } from '../src/runtime-config';
import {
  OpenTelemetryRuntime,
  openTelemetryInspectorView,
  type OpenTelemetrySdkHandle,
} from '../src/opentelemetry-runtime';
import { parseTelemetrySignal } from '../src/opentelemetry-signal';

interface ReceivedRequest {
  readonly path: string;
  readonly authorization: string | null;
  readonly contentType: string | null;
  readonly body: Uint8Array;
}

const servers: Bun.Server<unknown>[] = [];

afterEach(() => {
  for (const server of servers.splice(0)) server.stop(true);
});

async function fixture() {
  return await Bun.file(
    new URL('./fixtures/opentelemetry/contracts.v1.json', import.meta.url)
  ).json();
}

function configuration(
  base: ResolvedOpenTelemetryConfigurationV1,
  endpoint: string,
  overrides: Partial<ResolvedOpenTelemetryConfigurationV1> = {}
): ResolvedOpenTelemetryConfigurationV1 {
  return {
    ...base,
    enabled: true,
    signals: { traces: true, metrics: true, logs: true },
    endpoints: {
      traces: `${endpoint}/v1/traces`,
      metrics: `${endpoint}/v1/metrics`,
      logs: `${endpoint}/v1/logs`,
    },
    headerNames: {
      common: ['authorization'],
      traces: [],
      metrics: [],
      logs: [],
    },
    sampler: { name: 'always_on' },
    exportIntervalMs: 1_000,
    exportTimeoutMs: 500,
    shutdownTimeoutMs: 3_000,
    maxQueueSize: 2_048,
    maxExportBatchSize: 512,
    ...overrides,
  };
}

describe('OpenTelemetry Export v1 release journeys', () => {
  test('exports protobuf traces, metrics, and logs to a bounded local receiver', async () => {
    const requests: ReceivedRequest[] = [];
    const attempts = new Map<string, number>();
    const server = Bun.serve({
      hostname: '127.0.0.1',
      port: 0,
      async fetch(request) {
        const url = new URL(request.url);
        const body = new Uint8Array(await request.arrayBuffer());
        requests.push({
          path: url.pathname,
          authorization: request.headers.get('authorization'),
          contentType: request.headers.get('content-type'),
          body,
        });
        const attempt = (attempts.get(url.pathname) ?? 0) + 1;
        attempts.set(url.pathname, attempt);
        if (url.pathname === '/v1/traces' && attempt < 3) {
          return new Response(null, { status: 503 });
        }
        return new Response(new Uint8Array(), {
          status: 200,
          headers: { 'content-type': 'application/x-protobuf' },
        });
      },
    });
    servers.push(server);

    const contracts = await fixture();
    const secret = 'Bearer OTE8_RECEIVER_SECRET_7fd25a';
    const diagnostics: string[] = [];
    const health: unknown[] = [];
    const runtime = await OpenTelemetryRuntime.start(
      configuration(
        contracts.resolvedConfiguration as ResolvedOpenTelemetryConfigurationV1,
        `http://127.0.0.1:${server.port}`
      ),
      {
        runtimeInstanceId: 'runtime_ote8_receiver',
        deploymentId: 'deployment_ote8_receiver',
        runtimeVersion: '1.0.10',
        environment: {
          OTEL_EXPORTER_OTLP_HEADERS: `authorization=${encodeURIComponent(secret)}`,
        },
        onDiagnostic: (code, message) => diagnostics.push(`${code}:${message}`),
        onHealth: value => health.push(value),
      }
    );
    expect(runtime.record(parseTelemetrySignal(JSON.stringify(contracts.signal)))).toBe(true);
    await runtime.shutdown();

    const successfulPaths = new Set(
      requests
        .filter(request => (attempts.get(request.path) ?? 0) > 0)
        .map(request => request.path)
    );
    expect(successfulPaths).toEqual(
      new Set(['/v1/traces', '/v1/metrics', '/v1/logs'])
    );
    expect(attempts.get('/v1/traces')).toBeGreaterThanOrEqual(3);
    for (const request of requests) {
      expect(request.authorization).toBe(secret);
      expect(request.contentType).toContain('application/x-protobuf');
      expect(request.body.byteLength).toBeGreaterThan(0);
      const encoded = Buffer.from(request.body).toString('latin1');
      for (const forbidden of [
        'OTE8_RECEIVER_SECRET_7fd25a',
        'customer@example.test',
        'database-password',
        'raw-process-output',
      ]) {
        expect(encoded).not.toContain(forbidden);
      }
    }
    expect(runtime.health()).toMatchObject({
      state: 'stopped',
      queue: { size: 0, capacity: 2_048 },
    });
    const inspector = openTelemetryInspectorView({
      profile: 'woml.runtime-health/v2',
      openTelemetry: health.at(-1),
    });
    const publicSurfaces = JSON.stringify({
      diagnostics,
      health,
      inspector,
    });
    expect(publicSurfaces).not.toContain('OTE8_RECEIVER_SECRET_7fd25a');
    expect(publicSurfaces).not.toContain(secret);
  }, 15_000);

  test('bounds a ten-thousand-signal burst and restarts without retaining work', async () => {
    const contracts = await fixture();
    const base = contracts.resolvedConfiguration as ResolvedOpenTelemetryConfigurationV1;
    const config = configuration(base, 'http://127.0.0.1:4318', {
      signals: { traces: true, metrics: false, logs: false },
      headerNames: { common: [], traces: [], metrics: [], logs: [] },
      maxQueueSize: 2_048,
      maxExportBatchSize: 512,
    });
    const emitted: string[] = [];
    const loadSdk = async (): Promise<OpenTelemetrySdkHandle> => ({
      emit(signal) {
        emitted.push(signal.signalId);
        return true;
      },
      async forceFlush() {},
      async shutdown() {},
    });
    const start = () =>
      OpenTelemetryRuntime.start(config, {
        runtimeInstanceId: 'runtime_ote8_stress',
        deploymentId: 'deployment_ote8_stress',
        runtimeVersion: '1.0.10',
        environment: {},
        loadSdk,
      });

    const runtime = await start();
    const signal = parseTelemetrySignal(JSON.stringify(contracts.signal));
    let accepted = 0;
    for (let index = 0; index < 10_000; index += 1) {
      accepted += runtime.record({
        ...signal,
        signalId: `telemetry_stress_${index.toString().padStart(5, '0')}`,
        occurredAt: index % 2 === 0 ? '2000-01-01T00:00:00.000Z' : '2099-01-01T00:00:00.000Z',
      }) ? 1 : 0;
    }
    expect(accepted).toBe(config.maxQueueSize);
    expect(runtime.health().queue.size).toBe(config.maxQueueSize);
    expect(runtime.health().dropped).toBe(10_000 - config.maxQueueSize);
    await runtime.shutdown();
    expect(emitted).toHaveLength(config.maxQueueSize);
    expect(runtime.health()).toMatchObject({ state: 'stopped', queue: { size: 0 } });

    const restarted = await start();
    expect(restarted.record(signal)).toBe(true);
    await restarted.shutdown();
    expect(restarted.health()).toMatchObject({
      state: 'stopped',
      dropped: 0,
      queue: { size: 0 },
    });
  }, 10_000);
});
