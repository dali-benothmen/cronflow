import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-otel-package-'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged OpenTelemetry Export release', () => {
  test('a clean consumer loads the bundled optional host and exports without undeclared packages', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all([consumer, cache, temporary].map(path =>
      mkdir(path, { recursive: true })
    ));
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'otel-consumer', private: true })
    );
    const candidate = await installLocalReleaseCandidate(consumer, { cache, temporary });
    const manifest = await Bun.file(
      join(consumer, 'node_modules', 'woml-cli', 'package.json')
    ).json();
    expect(manifest.dependencies).toBeUndefined();
    expect(manifest.files).toContain('dist/opentelemetry-runtime.js');
    expect(manifest.files).toContain('dist/opentelemetry-sdk.js');
    const stagedFiles = await readdir(candidate.mainRoot, {
      recursive: true,
      withFileTypes: true,
    });
    for (const entry of stagedFiles) {
      if (!entry.isFile() || !/\.(?:js|json|md)$/u.test(entry.name)) continue;
      const path = join(entry.parentPath, entry.name);
      const source = await Bun.file(path).text();
      for (const forbidden of [
        'OTE8_RECEIVER_SECRET_7fd25a',
        'customer@example.test',
        'database-password',
        'raw-process-output',
      ]) {
        expect(source).not.toContain(forbidden);
      }
    }

    const requests: { path: string; bytes: number }[] = [];
    const server = Bun.serve({
      hostname: '127.0.0.1',
      port: 0,
      async fetch(request) {
        requests.push({
          path: new URL(request.url).pathname,
          bytes: (await request.arrayBuffer()).byteLength,
        });
        return new Response(new Uint8Array(), {
          headers: { 'content-type': 'application/x-protobuf' },
        });
      },
    });
    try {
      const runtimePath = join(
        consumer,
        'node_modules',
        'woml-cli',
        'dist',
        'opentelemetry-runtime.js'
      );
      const loaded = await import(pathToFileURL(runtimePath).href) as {
        readonly OpenTelemetryRuntime: {
          start(configuration: unknown, options: unknown): Promise<{
            record(signal: unknown): boolean;
            shutdown(): Promise<void>;
            health(): unknown;
          }>;
        };
      };
      const endpoint = `http://127.0.0.1:${server.port}`;
      const runtime = await loaded.OpenTelemetryRuntime.start(
        {
          profile: 'woml.opentelemetry-configuration/v1',
          enabled: true,
          protocol: 'http/protobuf',
          serviceName: 'packaged-woml',
          resourceAttributes: {},
          signals: { traces: true, metrics: false, logs: false },
          endpoints: {
            traces: `${endpoint}/v1/traces`,
            metrics: `${endpoint}/v1/metrics`,
            logs: `${endpoint}/v1/logs`,
          },
          headerNames: { common: [], traces: [], metrics: [], logs: [] },
          sampler: { name: 'always_on' },
          exportIntervalMs: 1_000,
          exportTimeoutMs: 500,
          shutdownTimeoutMs: 3_000,
          maxQueueSize: 256,
          maxExportBatchSize: 64,
          propagation: false,
          sources: {
            enabled: 'config',
            serviceName: 'default',
            resourceAttributes: 'default',
            traces: 'config',
            metrics: 'config',
            logs: 'config',
            sampler: 'config',
            endpoints: {
              traces: 'config',
              metrics: 'config',
              logs: 'config',
            },
          },
        },
        {
          runtimeInstanceId: 'runtime_packaged_otel',
          deploymentId: 'deployment_packaged_otel',
          runtimeVersion: '1.0.10',
          environment: {},
        }
      );
      expect(runtime.record({
        profile: 'woml.telemetry-signal/v1',
        signalId: 'telemetry_packaged_run_settled',
        kind: 'run',
        occurredAt: '2026-10-24T08:00:01.000Z',
        startedAt: '2026-10-24T08:00:00.000Z',
        endedAt: '2026-10-24T08:00:01.000Z',
        runtimeInstanceId: 'runtime_packaged_otel',
        deploymentId: 'deployment_packaged_otel',
        workflowId: 'packaged-otel',
        runId: 'run_packaged_otel',
        status: 'succeeded',
        durationMs: 1_000,
        attributes: { 'trigger.type': 'manual' },
      })).toBe(true);
      await runtime.shutdown();
      expect(requests).toEqual([
        { path: '/v1/traces', bytes: expect.any(Number) },
      ]);
      expect(requests[0]!.bytes).toBeGreaterThan(0);
      expect(runtime.health()).toMatchObject({ state: 'stopped' });
    } finally {
      server.stop(true);
    }
  }, 90_000);
});
