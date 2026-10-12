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

describe('Durable Artifacts local runtime journey', () => {
  test('stores, verifies, materializes, consumes, and cleans an artifact', async () => {
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
      <step id="consume" name="Consume durable report">
        <script>
          const artifact = context.steps.preserve.artifact;
          const metadata = await services.artifacts.head(artifact);
          const path = await services.artifacts.materialize(artifact);
          const consumed = await services.process.run({
            executable: "bun",
            args: ["-e", "console.log(await Bun.file(Bun.argv[1]).text())", path],
            timeout: "10s"
          }, { name: "consume-report" });
          return {
            artifact,
            integrity: metadata.integrity,
            content: consumed.stdout.trim()
          };
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
    expect(result.integrity).toBe('verified');
    expect(result.content).toBe('durable bytes');
    expect(
      Array.from(
        new Bun.Glob('**/*').scanSync({
          cwd: join(temporaryDirectory, 'materialized'),
          dot: true,
          onlyFiles: false,
        })
      )
    ).toEqual([]);

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
    const durableEvents = database
      .query('SELECT event_json AS eventJson FROM woml_run_events ORDER BY sequence')
      .all() as { eventJson: string }[];
    const artifactEvents = durableEvents.filter(({ eventJson }) =>
      eventJson.includes('"capability":"artifacts"')
    );
    expect(artifactEvents.length).toBeGreaterThanOrEqual(6);
    expect(artifactEvents.some(({ eventJson }) => eventJson.includes('"operation":"head"'))).toBe(true);
    expect(
      artifactEvents.some(({ eventJson }) =>
        eventJson.includes('"operation":"materialize"')
      )
    ).toBe(true);
    expect(artifactEvents.some(({ eventJson }) => eventJson.includes('"artifactId"'))).toBe(true);
    const durableHistory = durableEvents.map(({ eventJson }) => eventJson).join('\n');
    expect(durableHistory).not.toContain(join(temporaryDirectory, 'materialized'));
    expect(durableHistory).not.toContain(join(temporaryDirectory, 'artifact-bytes'));
    const runId = (
      database.query('SELECT run_id AS runId FROM woml_run_summaries LIMIT 1').get() as {
        runId: string;
      }
    ).runId;
    database.close();

    const inspected = run(
      'artifacts', 'inspect', result.artifact.id,
      '--state', state,
      '--json'
    );
    expect(inspected.exitCode, inspected.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(inspected.stdout?.toString() ?? '')).toMatchObject({
      profile: 'woml.artifact-inspection/v1',
      artifact: {
        id: result.artifact.id,
        name: 'release-report',
        size: 13,
      },
      digestPrefix: result.artifact.sha256.slice(0, 12),
      provenance: {
        workflowId: 'artifact-cli',
        runId,
        nodeId: 'preserve',
        attempt: 1,
      },
    });
    const inspectedRun = run('get', runId, '--state', state, '--json');
    expect(inspectedRun.exitCode, inspectedRun.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(inspectedRun.stdout?.toString() ?? '')).toMatchObject({
      profile: 'woml.run-inspection/v7',
      runId,
      artifacts: {
        profile: 'woml.artifact-run-inspection/v1',
        truncated: false,
        artifacts: [{ artifact: { id: result.artifact.id } }],
      },
    });
    const exportedPath = join(temporaryDirectory, 'operator-export.txt');
    const exported = run(
      'artifacts', 'export', result.artifact.id,
      '--output', exportedPath,
      '--artifact-policy', policy,
      '--state', state,
      '--json'
    );
    expect(exported.exitCode, exported.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(exported.stdout?.toString() ?? '')).toMatchObject({
      profile: 'woml.artifact-export/v1',
      artifactId: result.artifact.id,
      size: 13,
      verified: true,
    });
    expect(await Bun.file(exportedPath).text()).toBe('durable bytes');
    expect(exported.stdout?.toString() ?? '').not.toContain(exportedPath);

    const backupDirectory = join(temporaryDirectory, 'backup');
    const backup = run(
      'backup', backupDirectory,
      '--state', state,
      '--artifact-policy', policy,
      '--json'
    );
    expect(backup.exitCode, backup.stderr?.toString() ?? '').toBe(0);
    const manifest = JSON.parse(backup.stdout?.toString() ?? '');
    expect(manifest).toMatchObject({
      profile: 'woml.backup-manifest/v2',
      artifacts: {
        profile: 'woml.artifact-backup-inventory/v1',
        directory: 'artifacts',
        blobCount: 1,
        totalBytes: 13,
        inventoryDigest: expect.stringMatching(/^sha256:[0-9a-f]{64}$/),
        verified: true,
      },
      verified: true,
    });

    const restoredBackend = join(temporaryDirectory, 'restored-artifact-bytes');
    const restoredPolicy = join(temporaryDirectory, 'restored-artifact-policy.json');
    await writeFile(
      restoredPolicy,
      JSON.stringify({
        ...JSON.parse(await Bun.file(policy).text()),
        materialization: {
          directory: join(temporaryDirectory, 'restored-materialized'),
          cleanupOnStart: true,
          maxBytesPerAttempt: 1_048_576,
        },
        backend: {
          contract: 'woml.artifact-backend',
          contractVersion: 1,
          kind: 'local',
          directory: restoredBackend,
        },
      })
    );
    const restoredState = join(temporaryDirectory, 'restored/workflow-history.sqlite');
    const restored = run(
      'restore', backupDirectory,
      '--state', restoredState,
      '--artifact-policy', restoredPolicy,
      '--json'
    );
    expect(restored.exitCode, restored.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(restored.stdout?.toString() ?? '')).toMatchObject({
      profile: 'woml.restore-result/v1',
      backupId: manifest.backupId,
      statePath: restoredState,
    });
    expect(
      Array.from(
        new Bun.Glob('blobs/sha256/**/*').scanSync({
          cwd: restoredBackend,
          onlyFiles: true,
        })
      )
    ).toHaveLength(1);

    const expiring = new Database(state);
    expiring.run(
      "UPDATE woml_artifact_retention SET expires_at = '2000-01-01T00:00:00.000Z'"
    );
    expiring.close();
    const planned = run(
      'prune', '--before', '1h',
      '--state', state,
      '--artifact-policy', policy,
      '--dry-run',
      '--json'
    );
    expect(planned.exitCode, planned.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(planned.stdout?.toString() ?? '').artifacts).toMatchObject({
      kind: 'plan',
      eligibleArtifacts: 1,
      estimatedBytes: 13,
    });
    const pruned = run(
      'prune', '--before', '1h',
      '--state', state,
      '--artifact-policy', policy,
      '--json'
    );
    expect(pruned.exitCode, pruned.stderr?.toString() ?? '').toBe(0);
    expect(JSON.parse(pruned.stdout?.toString() ?? '').artifacts).toMatchObject({
      kind: 'result',
      deletedArtifacts: 1,
      deletedBlobs: 1,
      deletedBytes: 13,
      liveArtifacts: 0,
      liveBlobs: 0,
    });
  }, 30_000);
});
