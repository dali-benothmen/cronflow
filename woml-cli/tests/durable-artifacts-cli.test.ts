import { Database } from 'bun:sqlite';
import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const cli = resolve(import.meta.dir, '../dist/cli.js');
let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-artifacts-cli-'));
  await mkdir(join(temporaryDirectory, 'input'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

function run(...args: string[]): ReturnType<typeof Bun.spawnSync> {
  return Bun.spawnSync([process.execPath, cli, ...args], {
    cwd: temporaryDirectory,
    env: { ...process.env, NO_COLOR: '1' },
    stdout: 'pipe',
    stderr: 'pipe',
  });
}

describe('Durable Artifacts DA4 CLI journey', () => {
  test('creates a file through Process v1 and imports it through Rust', async () => {
    const state = join(temporaryDirectory, 'workflow-history.sqlite');
    const backend = join(temporaryDirectory, 'artifact-bytes');
    const policy = join(temporaryDirectory, 'artifact-policy.json');
    const workflow = join(temporaryDirectory, 'artifact.woml');
    await writeFile(
      policy,
      JSON.stringify({
        contract: 'woml.artifact-policy',
        contractVersion: 1,
        enabled: true,
        modes: { foreground: true, background: false },
        projectRoot: temporaryDirectory,
        readRoots: ['input'],
        materialization: {
          directory: join(temporaryDirectory, 'materialized'),
          cleanupOnStart: true,
          maxBytesPerAttempt: 1_048_576,
        },
        limits: {
          maxArtifactBytes: 1_048_576,
          maxRunBytes: 2_097_152,
          maxStoreBytes: 4_194_304,
          maxConcurrentTransfers: 2,
          transferTimeoutMs: 30_000,
        },
        retention: {
          defaultMs: 86_400_000,
          minimumMs: 3_600_000,
          maximumMs: 31_536_000_000,
        },
        secretScanning: {
          knownSecretAction: 'reject',
          minimumSecretBytes: 8,
        },
        backend: {
          contract: 'woml.artifact-backend',
          contractVersion: 1,
          kind: 'local',
          directory: backend,
        },
      })
    );
    await writeFile(
      workflow,
      `<woml>
  <workflow id="artifact-cli" name="Artifact CLI" version="1.0.0">
    <triggers><manual id="start" /></triggers>
    <steps>
      <step id="create" name="Create report">
        <script>
          await services.process.run({
            executable: "bun",
            args: ["-e", "await Bun.write('input/report.txt', 'durable bytes')"],
            timeout: "10s"
          }, { name: "create-report" });
          return { created: true };
        </script>
      </step>
      <step id="preserve" name="Preserve report">
        <script>
          const artifact = await services.artifacts.put("input/report.txt", {
            name: "release-report",
            contentType: "text/plain",
            retention: "24h"
          });
          return { artifact };
        </script>
      </step>
    </steps>
  </workflow>
</woml>\n`
    );

    const denied = run('test', workflow, '--allow-process', 'bun', '--state', state);
    expect(denied.exitCode).not.toBe(0);
    expect(denied.stderr?.toString() ?? '').toContain(
      'WOML_ARTIFACT_SERVICE_DISABLED'
    );

    const executed = run(
      'test', workflow,
      '--allow-process', 'bun',
      '--artifact-policy', policy,
      '--state', state
    );
    expect(executed.exitCode, executed.stderr?.toString() ?? '').toBe(0);
    const result = JSON.parse(executed.stdout?.toString() ?? '');
    expect(result.artifact).toMatchObject({
      contract: 'woml.artifact-reference',
      contractVersion: 1,
      name: 'release-report',
      size: 13,
      contentType: 'text/plain',
    });
    expect(result.artifact.id).toMatch(/^artifact_[0-9a-hjkmnp-tv-z]{26}$/);
    expect(result.artifact.sha256).toMatch(/^[0-9a-f]{64}$/);

    const database = new Database(state, { readonly: true });
    expect(
      database.query('SELECT COUNT(*) AS count FROM woml_artifacts').get()
    ).toEqual({ count: 1 });
    expect(
      database
        .query("SELECT status, operation FROM woml_artifact_transfers")
        .get()
    ).toEqual({ status: 'succeeded', operation: 'put' });
    expect(
      database.query('SELECT COUNT(*) AS count FROM woml_artifact_access').get()
    ).toEqual({ count: 1 });
    database.close();
  }, 30_000);
});
