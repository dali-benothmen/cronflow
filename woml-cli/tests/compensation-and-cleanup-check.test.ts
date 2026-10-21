import { describe, expect, test } from 'bun:test';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { runCli, type CliIo } from '../src/cli';

const valid = `<woml><workflow id="correction-check"><triggers><manual id="start" /></triggers><steps><saga id="provision"><step id="create"><script>return { id: "one" };</script><compensate retry="2"><script>return context.steps.create.id;</script></compensate></step><cleanup><step id="cleanup"><script>return true;</script></step></cleanup></saga></steps></workflow></woml>`;

async function command(...options: string[]) {
  const directory = await mkdtemp(join(tmpdir(), 'woml-correction-check-'));
  const file = join(directory, 'workflow.woml');
  const stdout: string[] = [];
  const stderr: string[] = [];
  const io: CliIo = {
    stdout: text => stdout.push(text),
    stderr: text => stderr.push(text),
  };
  try {
    await writeFile(file, valid, 'utf8');
    const exitCode = await runCli(['check', file, ...options], io);
    return { exitCode, stdout: stdout.join(''), stderr: stderr.join('') };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

describe('woml check compensation and cleanup authoring', () => {
  test('accepts the frozen Model v19 authoring surface', async () => {
    const result = await command();
    expect(result.exitCode).toBe(0);
    expect(result.stderr).toBe('');
    expect(result.stdout).toContain('WOML check passed');
    expect(result.stdout).toContain('Compiled Model v19');
    expect(result.stdout).toContain('CC2–CC4');
  });

  test('returns the Model v19 frontend profile as JSON', async () => {
    const result = await command('--json');
    expect(result.exitCode).toBe(0);
    expect(JSON.parse(result.stdout)).toEqual({
      profile: 'woml.model-validation/v1',
      valid: true,
      workflowId: 'correction-check',
      feature: 'compensation-and-cleanup',
      executable: false,
      modelVersion: 19,
      runtimePhase: 'CC1',
    });
  });
});
