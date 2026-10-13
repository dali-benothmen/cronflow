import { afterEach, describe, expect, test } from 'bun:test';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { inspectRunV2WithRust, inspectWaitsV1WithRust } from '../src/rust-executor';

const packageRoot = resolve(import.meta.dir, '..');
const projectRoot = resolve(packageRoot, '..');
const executable = join(packageRoot, 'dist', 'cli.js');
const nativeCorePath = join(
  packageRoot,
  'dist',
  `woml-core.${process.platform}-${process.arch}.node`
);
const reviewedFixture = join(
  projectRoot,
  'woml/tests/fixtures/durable-waits/composition.reviewed.woml'
);
const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map(path => rm(path, { recursive: true, force: true }))
  );
});

function availablePort(): number {
  const server = Bun.serve({
    hostname: '127.0.0.1',
    port: 0,
    fetch: () => new Response(),
  });
  const port = server.port;
  server.stop(true);
  if (port === undefined) throw new Error('Bun did not assign a local port.');
  return port;
}

async function waitFor(read: () => string, expected: string): Promise<void> {
  const deadline = Date.now() + 20_000;
  while (!read().includes(expected)) {
    if (Date.now() >= deadline) throw new Error(read());
    await Bun.sleep(10);
  }
}

describe('Durable wait control-flow composition', () => {
  test('resumes and settles waits composed through choose, parallel, and fork', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'woml-dw6-composition-'));
    temporaryDirectories.push(directory);
    const workflowPath = join(directory, 'composition.woml');
    const statePath = join(directory, 'state.sqlite');
    const source = (await readFile(reviewedFixture, 'utf8')).replace(
      '<manual id="start" />',
      '<webhook id="start" path="/dw6-composition" method="POST" auth="none" />'
    );
    await writeFile(workflowPath, source, 'utf8');
    const port = availablePort();

    const child = Bun.spawn(
      [
        executable,
        'run',
        workflowPath,
        '--state',
        statePath,
        '--port',
        String(port),
        '--color=never',
      ],
      {
        cwd: projectRoot,
        stdout: 'pipe',
        stderr: 'pipe',
      }
    );
    let stderr = '';
    const stderrDone = (async () => {
      const reader = child.stderr.getReader();
      const decoder = new TextDecoder();
      while (true) {
        const chunk = await reader.read();
        if (chunk.done) break;
        stderr += decoder.decode(chunk.value, { stream: true });
      }
      stderr += decoder.decode();
    })();

    try {
      await waitFor(() => stderr, 'WOML automation is active');
      const response = await fetch(`http://127.0.0.1:${port}/dw6-composition`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: '{}',
      });
      expect(response.status).toBe(202);
      await waitFor(() => stderr, 'RUN COMPLETED');
      const runId = stderr.match(/run_[a-f0-9]{32}/)?.[0];
      if (runId === undefined) throw new Error(stderr);

      const run = inspectRunV2WithRust(statePath, runId, { nativeCorePath });
      const waits = inspectWaitsV1WithRust(statePath, runId, { nativeCorePath });
      expect(run.status).toBe('succeeded');
      expect(waits.waits.map(wait => [wait.waitId, wait.status])).toEqual([
        ['choicePause', 'elapsed'],
        ['parallelTimer', 'elapsed'],
        ['forkTimer', 'elapsed'],
      ]);
    } finally {
      if (child.exitCode === null) child.kill('SIGINT');
      await child.exited;
      await stderrDone;
    }
  }, 30_000);
});
