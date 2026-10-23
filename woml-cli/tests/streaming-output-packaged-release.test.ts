import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(join(tmpdir(), 'woml-streaming-package-'));
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged Streaming Output and Progress release', () => {
  test('a clean consumer can stream, replay, filter, reconnect, and inspect output', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all(
      [consumer, cache, temporary].map(path => mkdir(path, { recursive: true }))
    );
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'streaming-output-consumer', private: true })
    );
    await Bun.write(
      join(consumer, 'streaming.woml'),
      `<woml>
  <workflow id="packaged-streaming" name="Packaged streaming" version="1.0.0">
    <triggers><manual id="start" /></triggers>
    <steps>
      <step id="prepare" name="Report preparation"><script>
        await services.progress.report({
          id: "files", current: 1, total: 2, unit: "files",
          message: "Prepared packaged input"
        });
        return { ready: true };
      </script></step>
      <step id="execute" name="Stream the packaged process"><script>
        const result = await services.process.run({
          executable: "bun",
          args: ["-e", "process.stdout.write('packaged-stream\\\\n'); process.stderr.write('packaged-warning\\\\n')"],
          stream: true,
          timeout: "10s",
          output: { stdout: "capture", stderr: "capture", maxBytes: 4096 }
        }, { name: "packaged-stream" });
        return { exitCode: result.exitCode, streamed: result.streamed };
      </script></step>
    </steps>
  </workflow>
</woml>`
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

    const checked = invoke('check', 'streaming.woml');
    expect(checked.exitCode, checked.stderr.toString()).toBe(0);
    const executed = invoke(
      'test',
      'streaming.woml',
      '--allow-process',
      'bun',
      '--state',
      state
    );
    expect(executed.exitCode, executed.stderr.toString()).toBe(0);
    expect(JSON.parse(executed.stdout.toString())).toEqual({ exitCode: 0, streamed: true });

    const listed = invoke('list', '--state', state, '--json');
    expect(listed.exitCode, listed.stderr.toString()).toBe(0);
    const runId = JSON.parse(listed.stdout.toString()).runs[0].runId as string;
    const replayed = invoke(
      'logs', runId, '--state', state, '--no-follow', '--phase', 'forward', '--json'
    );
    expect(replayed.exitCode, replayed.stderr.toString()).toBe(0);
    const records = replayed.stdout
      .toString()
      .trim()
      .split('\n')
      .filter(Boolean)
      .map(line => JSON.parse(line));
    expect(records.some(record => record.kind === 'progress' && record.progressId === 'files')).toBe(true);
    expect(records.some(record => record.kind === 'output' && record.source === 'stdout' && record.text.includes('packaged-stream'))).toBe(true);
    expect(records.some(record => record.kind === 'output' && record.source === 'stderr' && record.text.includes('packaged-warning'))).toBe(true);
    expect(records.some(record => record.kind === 'closed')).toBe(true);
    expect(records.every(record => record.profile === 'woml.operational-record/v1')).toBe(true);

    const after = records.find(record => record.kind === 'progress')!.sequence as number;
    const resumed = invoke(
      'logs', runId, '--state', state, '--after', String(after), '--no-follow', '--json'
    );
    expect(resumed.exitCode, resumed.stderr.toString()).toBe(0);
    const resumedRecords = resumed.stdout
      .toString()
      .trim()
      .split('\n')
      .filter(Boolean)
      .map(line => JSON.parse(line));
    expect(resumedRecords.every(record => Number(record.sequence) > after)).toBe(true);

    const inspected = invoke('get', runId, '--state', state, '--json');
    expect(inspected.exitCode, inspected.stderr.toString()).toBe(0);
    expect(JSON.parse(inspected.stdout.toString())).toMatchObject({
      runId,
      status: 'succeeded',
    });
  }, 90_000);
});
