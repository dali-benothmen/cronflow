import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-correction-package-'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged Compensation and Cleanup release', () => {
  test('a clean consumer validates and executes a real corrective workflow', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all(
      [consumer, cache, temporary].map(path => mkdir(path, { recursive: true }))
    );
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'correction-consumer', private: true })
    );
    await Bun.write(
      join(consumer, 'correction.woml'),
      `<woml><workflow id="packaged-correction" version="1.0.0"><triggers><manual id="start" /></triggers><steps><saga id="provision"><step id="createResource"><script>return { resourceId: "resource-42" };</script><compensate retry="2"><script>return { removedResource: context.steps.createResource.resourceId, reason: context.compensation.reason };</script></compensate></step><step id="failProvision"><script>throw new Error("intentional packaged correction");</script></step><cleanup><step id="recordCleanup"><script>return { cleaned: true, status: context.cleanup.compensationStatus };</script></step></cleanup></saga></steps></workflow></woml>`
    );

    await installLocalReleaseCandidate(consumer, { cache, temporary });
    const executable = join(consumer, 'node_modules', '.bin', 'woml');
    const state = join(consumer, 'workflow-history.sqlite');
    const invoke = (...args: string[]) =>
      Bun.spawnSync([executable, ...args], {
        cwd: consumer,
        stdout: 'pipe',
        stderr: 'pipe',
        env: { ...process.env, NO_COLOR: '1' },
      });

    const checked = invoke('check', 'correction.woml', '--json');
    expect(checked.exitCode, checked.stderr.toString()).toBe(0);
    expect(checked.stdout.toString()).toContain('packaged-correction');

    const execution = invoke('test', 'correction.woml', '--state', state);
    expect(execution.exitCode).toBe(1);

    const listing = invoke(
      'list', '--workflow', 'packaged-correction', '--status', 'failed',
      '--limit', '1', '--state', state, '--json'
    );
    expect(listing.exitCode, listing.stderr.toString()).toBe(0);
    const runId = JSON.parse(listing.stdout.toString()).runs[0]?.runId as string;
    expect(runId).toMatch(/^run_/);

    const inspection = invoke('get', runId, '--state', state, '--json');
    expect(inspection.exitCode, inspection.stderr.toString()).toBe(0);
    expect(JSON.parse(inspection.stdout.toString())).toMatchObject({
      profile: 'woml.run-inspection/v10',
      status: 'failed',
      businessOutcome: 'failed',
      correction: {
        profile: 'woml.correction-inspection/v1',
        phase: 'completed',
        originalOutcome: 'failed',
        sagas: [{
          sagaId: 'provision',
          registeredCompensations: 1,
          completedCompensations: 1,
          completedCleanup: 1,
        }],
        unresolvedActions: [],
      },
    });

    const human = invoke('get', runId, '--state', state);
    expect(human.exitCode, human.stderr.toString()).toBe(0);
    expect(human.stdout.toString()).toContain('Correction:');
    expect(human.stdout.toString()).toContain('1/1 compensated');
    expect(human.stdout.toString()).toContain('1 cleanup completed');
  }, 90_000);
});
