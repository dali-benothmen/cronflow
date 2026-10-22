import { resolve } from 'node:path';

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';

import { runCli, type CliIo } from '../src/cli';

const repositoryRoot = resolve(import.meta.dir, '../..');
const originalWorkingDirectory = process.cwd();
const policy = resolve(
  repositoryRoot,
  'woml/tests/fixtures/infrastructure-policies/production.unsigned.v1.json'
);

function capture(): { io: CliIo; stdout: string[]; stderr: string[] } {
  const stdout: string[] = [];
  const stderr: string[] = [];
  return {
    stdout,
    stderr,
    io: {
      stdout: text => stdout.push(text),
      stderr: text => stderr.push(text),
      isTTY: false,
    },
  };
}

describe('Infrastructure Policy CLI', () => {
  beforeAll(() => process.chdir(repositoryRoot));
  afterAll(() => process.chdir(originalWorkingDirectory));

  test('checks a policy with stable machine-readable identity', async () => {
    const output = capture();
    expect(await runCli(['policy', 'check', policy, '--json'], output.io)).toBe(
      0
    );
    expect(output.stderr.join('')).toBe('');
    expect(JSON.parse(output.stdout.join(''))).toMatchObject({
      profile: 'woml.infrastructure-policy-check/v1',
      status: 'valid',
      id: 'production',
      environment: 'production',
      signatureStatus: 'unsigned',
    });
  });

  test('explains dynamic service operations without promising runtime authorization', async () => {
    const output = capture();
    const workflow = resolve(
      repositoryRoot,
      'woml/tests/fixtures/services-bindings.woml'
    );
    const exit = await runCli(
      ['policy', 'explain', workflow, '--policy', policy, '--json'],
      output.io
    );
    expect(exit).toBe(0);
    const report = JSON.parse(output.stdout.join('')) as {
      operations: { status: string }[];
      guarantee: string;
    };
    expect(
      report.operations.every(operation => operation.status !== 'allowed')
    ).toBe(true);
    expect(report.guarantee).toContain('advisory');
  });
});
