import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { installLocalReleaseCandidate } from './helpers/release-candidate';

let temporaryDirectory: string;

beforeAll(async () => {
  temporaryDirectory = await mkdtemp(
    join(tmpdir(), 'woml-infrastructure-policy-package-')
  );
});

afterAll(async () => {
  await rm(temporaryDirectory, { recursive: true, force: true });
});

describe('Packaged Infrastructure Policies release', () => {
  test('a clean consumer can check and explain policy while production requirements fail closed', async () => {
    const consumer = join(temporaryDirectory, 'consumer');
    const cache = join(temporaryDirectory, 'cache');
    const temporary = join(temporaryDirectory, 'tmp');
    await Promise.all(
      [consumer, cache, temporary].map(path => mkdir(path, { recursive: true }))
    );
    await Bun.write(
      join(consumer, 'package.json'),
      JSON.stringify({ name: 'infrastructure-policy-consumer', private: true })
    );
    await Bun.write(
      join(consumer, 'deployment.woml'),
      await readFile(
        join(
          import.meta.dir,
          '../../examples/infrastructurePolicies/reviewedTerraformRelease.woml'
        ),
        'utf8'
      )
    );
    await Bun.write(
      join(consumer, 'production.json'),
      await readFile(
        join(
          import.meta.dir,
          '../../examples/infrastructurePolicies/production-policy.example.json'
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

    const checked = invoke('policy', 'check', 'production.json', '--json');
    expect(checked.exitCode, checked.stderr.toString()).toBe(0);
    expect(JSON.parse(checked.stdout.toString())).toMatchObject({
      profile: 'woml.infrastructure-policy-check/v1',
      status: 'valid',
      id: 'reviewed-production',
      environment: 'production',
    });

    const explained = invoke(
      'policy',
      'explain',
      'deployment.woml',
      '--policy',
      'production.json',
      '--json'
    );
    expect(explained.exitCode, explained.stderr.toString()).toBe(0);
    const report = JSON.parse(explained.stdout.toString()) as {
      guarantee: string;
      operations: { status: string }[];
    };
    expect(report.guarantee).toContain('advisory');
    expect(report.operations.some(item => item.status === 'unknown_until_runtime')).toBe(true);

    const missing = invoke(
      'check',
      'deployment.woml',
      '--require-policy-signature'
    );
    expect(missing.exitCode).not.toBe(0);
    expect(`${missing.stdout}${missing.stderr}`).toContain('WOML_POLICY_REQUIRED');

    const unsigned = invoke(
      'policy',
      'check',
      'production.json',
      '--require-signature'
    );
    expect(unsigned.exitCode).not.toBe(0);
    expect(`${unsigned.stdout}${unsigned.stderr}`).toContain(
      'WOML_POLICY_SIGNATURE_INVALID'
    );
  }, 90_000);
});
