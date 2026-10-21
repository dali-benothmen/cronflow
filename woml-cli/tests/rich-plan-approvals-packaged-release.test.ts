import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-rich-approval-package-'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged Rich Plan Approvals release', () => {
  test('a clean consumer validates rich approval authoring and fails closed without reviewer authority', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all(
      [consumer, cache, temporary].map(path => mkdir(path, { recursive: true }))
    );
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'rich-approval-consumer', private: true })
    );
    await Bun.write(
      join(consumer, 'review.woml'),
      await readFile(
        join(
          import.meta.dir,
          '../../woml/tests/fixtures/rich-plan-approvals/model.reviewed.woml'
        ),
        'utf8'
      )
    );

    await installLocalReleaseCandidate(consumer, { cache, temporary });
    const executable = join(consumer, 'node_modules', '.bin', 'woml');
    const invoke = (...args: string[]) =>
      Bun.spawnSync([executable, ...args], {
        cwd: consumer,
        stdout: 'pipe',
        stderr: 'pipe',
        env: { ...process.env, NO_COLOR: '1' },
      });

    const checked = invoke('check', 'review.woml', '--json');
    expect(checked.exitCode, checked.stderr.toString()).toBe(0);
    expect(checked.stdout.toString()).toContain('production-deployment');

    const execution = invoke(
      'test', 'review.woml', '--state', 'review.sqlite'
    );
    expect(execution.exitCode).not.toBe(0);
    expect(`${execution.stdout}${execution.stderr}`).toContain(
      'WOML_APPROVAL_AUTHORITY_UNAVAILABLE'
    );
  }, 90_000);
});
