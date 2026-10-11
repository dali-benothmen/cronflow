#!/usr/bin/env bun

import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const cli = resolve(import.meta.dir, '../dist/cli.js');
const iterations = Number.parseInt(
  process.argv
    .find(value => value.startsWith('--iterations='))
    ?.split('=', 2)[1] ?? '10',
  10
);
if (!Number.isSafeInteger(iterations) || iterations < 1 || iterations > 100) {
  throw new Error('--iterations must be an integer from 1 through 100.');
}

function median(values: readonly number[]): number {
  const ordered = [...values].sort((left, right) => left - right);
  const middle = Math.floor(ordered.length / 2);
  return ordered.length % 2 === 0
    ? (ordered[middle - 1]! + ordered[middle]!) / 2
    : ordered[middle]!;
}

const directory = await mkdtemp(join(tmpdir(), 'woml-process-benchmark-'));
try {
  await writeFile(join(directory, 'noop.ts'), 'process.stdout.write("ok");\n');
  await writeFile(
    join(directory, 'process.woml'),
    `<woml>
  <workflow id="process-dispatch-benchmark" name="Process dispatch benchmark" version="1.0.0">
    <triggers><manual id="start" /></triggers>
    <steps><step id="run"><script>
      const result = await services.process.run({
        executable: "bun",
        args: ["run", "noop.ts"],
        timeout: "10s"
      }, { name: "benchmark-noop" });
      return { childDurationMs: result.durationMs };
    </script></step></steps>
  </workflow>
</woml>\n`
  );

  const samples: { wallMs: number; childMs: number }[] = [];
  for (let index = -2; index < iterations; index += 1) {
    const started = performance.now();
    const result = Bun.spawnSync(
      [
        process.execPath,
        cli,
        'test',
        'process.woml',
        '--allow-process',
        'bun',
        '--state',
        join(directory, `state-${index}.sqlite`),
      ],
      { cwd: directory, stdout: 'pipe', stderr: 'pipe' }
    );
    const wallMs = performance.now() - started;
    if (result.exitCode !== 0) {
      throw new Error(result.stderr.toString());
    }
    const childMs = Number(
      JSON.parse(result.stdout.toString()).childDurationMs
    );
    if (index >= 0) samples.push({ wallMs, childMs });
  }

  const wall = median(samples.map(sample => sample.wallMs));
  const child = median(samples.map(sample => sample.childMs));
  const residual = median(
    samples.map(sample => sample.wallMs - sample.childMs)
  );
  process.stdout.write(`WOML PROCESS DISPATCH BENCHMARK v1\n\n`);
  process.stdout.write(
    `Samples                 ${iterations} measured · 2 warm-up\n`
  );
  process.stdout.write(`End-to-end wall median  ${wall.toFixed(2)} ms\n`);
  process.stdout.write(`Child runtime median    ${child.toFixed(2)} ms\n`);
  process.stdout.write(`Observed residual       ${residual.toFixed(2)} ms\n\n`);
  process.stdout.write(
    'The residual includes CLI startup, compilation/cache lookup, durable events, script workers, serialization, native dispatch, and measurement skew. It is diagnostic context, not a claim that all residual time is Process v1 overhead.\n'
  );
} finally {
  await rm(directory, { recursive: true, force: true });
}
