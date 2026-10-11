import { mkdir, mkdtemp, realpath, symlink } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

import { describe, expect, test } from 'bun:test';

import {
  createForegroundProcessGrant,
  preflightProcessPolicy,
  ProcessPolicyError,
} from '../src/process-policy';

describe('Process Policy v1 preflight', () => {
  test('canonicalizes approved executables and working directories', async () => {
    const root = await mkdtemp(join(tmpdir(), 'woml-process-policy-'));
    await mkdir(join(root, 'workspace'));
    const bun = Bun.which('bun');
    expect(bun).not.toBeNull();
    const policy = await preflightProcessPolicy(
      {
        contract: 'woml.process-policy',
        contractVersion: 1,
        enabled: true,
        modes: { foreground: true, background: false },
        projectRoot: root,
        executables: [{ alias: 'bun', path: bun! }],
        workingDirectories: ['workspace'],
        environment: { inherit: ['PATH'], allow: ['CI'] },
        limits: {
          maxTimeoutMs: 30_000,
          maxOutputBytes: 65_536,
          maxConcurrent: 2,
        },
      },
      root,
      'foreground'
    );
    expect(policy.projectRoot).toBe(await realpath(root));
    expect(policy.executables[0]?.path).toBe(await realpath(bun!));
    expect(policy.workingDirectories).toEqual([
      await realpath(join(root, 'workspace')),
    ]);
  });

  test('rejects a working-directory symlink escape', async () => {
    if (process.platform === 'win32') return;
    const root = await mkdtemp(join(tmpdir(), 'woml-process-policy-'));
    const outside = await mkdtemp(join(tmpdir(), 'woml-process-outside-'));
    await symlink(outside, join(root, 'escape'), 'dir');
    const bun = Bun.which('bun')!;
    await expect(
      preflightProcessPolicy(
        {
          contract: 'woml.process-policy',
          contractVersion: 1,
          enabled: true,
          modes: { foreground: true, background: false },
          projectRoot: root,
          executables: [{ alias: 'bun', path: bun }],
          workingDirectories: ['escape'],
          environment: { inherit: [], allow: [] },
          limits: {
            maxTimeoutMs: 1,
            maxOutputBytes: 1,
            maxConcurrent: 1,
          },
        },
        root,
        'foreground'
      )
    ).rejects.toMatchObject({ code: 'WOML_PROCESS_CWD_DENIED' });
  });

  test('foreground shorthand resolves aliases and never enables background mode', async () => {
    const root = await mkdtemp(join(tmpdir(), 'woml-process-grant-'));
    const grant = await createForegroundProcessGrant('bun', root);
    expect(grant.process.mode).toBe('foreground');
    expect(grant.process.policy.modes).toEqual({
      foreground: true,
      background: false,
    });
    expect(grant.process.policy.executables[0]?.alias).toBe('bun');
  });

  test('rejects unknown fields and disabled policies with stable codes', async () => {
    const root = await mkdtemp(join(tmpdir(), 'woml-process-policy-'));
    const base = {
      contract: 'woml.process-policy',
      contractVersion: 1,
      enabled: true,
      modes: { foreground: true, background: false },
      projectRoot: root,
      executables: [],
      workingDirectories: ['.'],
      environment: { inherit: [], allow: [] },
      limits: { maxTimeoutMs: 1, maxOutputBytes: 1, maxConcurrent: 1 },
    };
    await expect(
      preflightProcessPolicy({ ...base, surprise: true }, root, 'foreground')
    ).rejects.toBeInstanceOf(ProcessPolicyError);
    await expect(
      preflightProcessPolicy({ ...base, enabled: false }, root, 'foreground')
    ).rejects.toMatchObject({ code: 'WOML_PROCESS_DISABLED' });
  });
});
