import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-waits-package-'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged Durable Waits release', () => {
  test('a clean consumer can execute a timer and compile an external callback', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all(
      [consumer, cache, temporary].map(path => mkdir(path, { recursive: true }))
    );
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'durable-wait-consumer', private: true })
    );
    await Bun.write(
      join(consumer, 'timer.woml'),
      `<woml><workflow id="packaged-timer" version="1.0.0"><triggers><manual id="start" /></triggers><steps><wait id="pause" duration="1s" /><step id="done"><script>return { status: context.steps.pause.status };</script></step></steps></workflow></woml>`
    );
    await Bun.write(
      join(consumer, 'callback.woml'),
      `<woml><workflow id="packaged-callback" version="1.0.0"><triggers><manual id="start" /></triggers><steps><step id="begin"><script>return { id: "job-42" };</script></step><wait-for id="result" signal="job.completed" correlation="{{context.steps.begin.id}}" timeout="1h"><schema>{"type":"object","required":["ok"]}</schema></wait-for><step id="done"><script>return context.steps.result.payload;</script></step></steps></workflow></woml>`
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

    const timer = invoke('test', 'timer.woml', '--state', 'timer.sqlite');
    expect(timer.exitCode, timer.stderr.toString()).toBe(0);
    expect(timer.stdout.toString()).toContain('"status":"elapsed"');

    const callback = invoke('check', 'callback.woml', '--json');
    expect(callback.exitCode, callback.stderr.toString()).toBe(0);
    expect(callback.stdout.toString()).toContain('packaged-callback');

    const help = invoke('--help');
    expect(help.exitCode, help.stderr.toString()).toBe(0);
    expect(help.stdout.toString()).toContain('woml signal <name>');
  }, 90_000);
});
