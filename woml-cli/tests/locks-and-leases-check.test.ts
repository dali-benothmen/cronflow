import { describe, expect, test } from 'bun:test';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { runCli, type CliIo } from '../src/cli';

const valid = `<woml>
  <workflow id="lock-check">
    <triggers><manual id="start" /></triggers>
    <steps>
      <step id="prepare"><script>return { environment: "production" };</script></step>
      <lock id="deployment" key="deploy/{{context.steps.prepare.environment}}" acquire-timeout="30s" lease="30s">
        <step id="deploy"><script>return { deployed: true };</script></step>
      </lock>
      <step id="done"><script>return context.steps.deployment;</script></step>
    </steps>
  </workflow>
</woml>`;

async function command(command: 'check' | 'run', ...options: string[]) {
  const directory = await mkdtemp(join(tmpdir(), 'woml-lock-check-'));
  const file = join(directory, 'workflow.woml');
  const stdout: string[] = [];
  const stderr: string[] = [];
  const io: CliIo = {
    stdout: text => stdout.push(text),
    stderr: text => stderr.push(text),
  };
  try {
    await writeFile(file, valid, 'utf8');
    const exitCode = await runCli([command, file, ...options], io);
    return { exitCode, stdout: stdout.join(''), stderr: stderr.join('') };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

describe('woml check lock authoring', () => {
  test('accepts a valid Model v18 lock while identifying its deferred runtime', async () => {
    const result = await command('check');
    expect(result.exitCode).toBe(0);
    expect(result.stderr).toBe('');
    expect(result.stdout).toContain('WOML check passed');
    expect(result.stdout).toContain('Compiled Model v18');
    expect(result.stdout).toContain('Durable execution begins in LL2–LL4');
  });

  test('returns the frozen frontend-validation profile as JSON', async () => {
    const result = await command('check', '--json');
    expect(result.exitCode).toBe(0);
    expect(result.stderr).toBe('');
    expect(JSON.parse(result.stdout)).toEqual({
      profile: 'woml.model-validation/v1',
      valid: true,
      workflowId: 'lock-check',
      feature: 'locks-and-leases',
      executable: false,
      modelVersion: 18,
      runtimePhase: 'LL2–LL4',
    });
  });

  test('fails closed before runtime activation until Rust owns locks', async () => {
    const result = await command('run');
    expect(result.exitCode).toBe(1);
    expect(result.stdout).toBe('');
    expect(result.stderr).toContain('WOML_LOCK_RUNTIME_UNAVAILABLE');
    expect(result.stderr).toContain('woml check');
  });
});
