import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-locks-package-'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged Locks and Leases release', () => {
  test('a clean consumer validates and executes a durable lock workflow', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all(
      [consumer, cache, temporary].map(path => mkdir(path, { recursive: true }))
    );
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'locks-consumer', private: true })
    );
    await Bun.write(
      join(consumer, 'lock.woml'),
      `<woml><workflow id="packaged-lock" version="1.0.0"><triggers><manual id="start" /></triggers><steps><step id="plan"><script>return { resource: "customer-42" };</script></step><lock id="reservation" key="customer/{{context.steps.plan.resource}}" acquire-timeout="5s" lease="5s"><step id="reserve"><script>return { key: context.lock.key, fencingToken: context.lock.fencingToken };</script></step></lock><step id="done"><script>return context.steps.reservation;</script></step></steps></workflow></woml>`
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

    const checked = invoke('check', 'lock.woml', '--json');
    expect(checked.exitCode, checked.stderr.toString()).toBe(0);
    expect(checked.stdout.toString()).toContain('packaged-lock');

    const run = invoke('test', 'lock.woml', '--state', 'locks.sqlite');
    expect(run.exitCode, run.stderr.toString()).toBe(0);
    expect(run.stdout.toString()).toContain('fencingToken');

    const help = invoke('--help');
    expect(help.exitCode, help.stderr.toString()).toBe(0);
    expect(help.stdout.toString()).toContain('lock/lease state');
  }, 90_000);
});
