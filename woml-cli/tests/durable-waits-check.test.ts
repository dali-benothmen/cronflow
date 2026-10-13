import { describe, expect, test } from 'bun:test';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { runCli, type CliIo } from '../src/cli';

const valid = `<woml>
  <workflow id="durable-wait-check">
    <triggers><manual id="start" /></triggers>
    <steps>
      <step id="begin"><script>return { jobId: "job-1" };</script></step>
      <wait-for id="result" signal="job.completed" correlation="{{context.steps.begin.jobId}}" timeout="1h" />
      <step id="done"><script>return context.steps.result;</script></step>
    </steps>
  </workflow>
</woml>`;

async function check(source: string, ...options: string[]) {
  const directory = await mkdtemp(join(tmpdir(), 'woml-durable-wait-check-'));
  const file = join(directory, 'workflow.woml');
  const stdout: string[] = [];
  const stderr: string[] = [];
  const io: CliIo = {
    stdout: text => stdout.push(text),
    stderr: text => stderr.push(text),
  };
  try {
    await writeFile(file, source, 'utf8');
    await writeFile(
      join(directory, 'jobs.ts'),
      'export function id(value: string) { return value; }\n',
      'utf8'
    );
    const exitCode = await runCli(['check', file, ...options], io);
    return { exitCode, stdout: stdout.join(''), stderr: stderr.join('') };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

describe('woml check durable-wait authoring', () => {
  test('accepts valid Model v17 source without claiming runtime execution', async () => {
    const result = await check(valid);
    expect(result.exitCode).toBe(0);
    expect(result.stderr).toBe('');
    expect(result.stdout).toContain('WOML check passed');
    expect(result.stdout).toContain('Compiled Model v17');
    expect(result.stdout).toContain('execution begins in DW2');
  });

  test('returns the frozen frontend-validation profile as JSON', async () => {
    const result = await check(valid, '--json');
    expect(result.exitCode).toBe(0);
    expect(JSON.parse(result.stdout)).toEqual({
      profile: 'woml.model-validation/v1',
      valid: true,
      workflowId: 'durable-wait-check',
      feature: 'durable-waits',
      executable: false,
      modelVersion: 17,
      runtimePhase: 'DW2',
    });
  });

  test('keeps local modules available in a Model v17 wait workflow', async () => {
    const source = valid
      .replace(
        '<workflow id="durable-wait-check">',
        '<imports><module name="jobs" from="./jobs.ts" /></imports><workflow id="durable-wait-check">'
      )
      .replace(
        'return { jobId: "job-1" };',
        'return { jobId: services.jobs.id("job-1") };'
      );
    const result = await check(source, '--json');
    expect(result.exitCode).toBe(0);
    expect(JSON.parse(result.stdout)).toMatchObject({
      valid: true,
      feature: 'durable-waits',
      modelVersion: 17,
    });
  });

  test('preserves source-located diagnostics for invalid waits', async () => {
    const result = await check(valid.replace('timeout="1h"', 'timeout="soon"'));
    expect(result.exitCode).toBe(1);
    expect(result.stdout).toBe('');
    expect(result.stderr).toContain('WOML_WAIT_DURATION_INVALID');
    expect(result.stderr).toContain('workflow.woml:');
  });
});
