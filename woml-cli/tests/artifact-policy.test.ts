import { mkdir, mkdtemp, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, test } from 'bun:test';

import {
  artifactPolicySecretNames,
  ArtifactPolicyError,
  preflightArtifactPolicy,
} from '../src/artifact-policy';

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map(path =>
      rm(path, { recursive: true, force: true })
    )
  );
});

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'woml-artifact-policy-'));
  temporaryDirectories.push(root);
  await mkdir(join(root, 'reports'));
  return {
    root,
    policy: {
      contract: 'woml.artifact-policy',
      contractVersion: 1,
      enabled: true,
      modes: { foreground: true, background: false },
      projectRoot: '.',
      readRoots: ['.', 'reports'],
      materialization: {
        directory: '.woml/materialized-v1',
        cleanupOnStart: true,
        maxBytesPerAttempt: 2_147_483_648,
      },
      limits: {
        maxArtifactBytes: 1_073_741_824,
        maxRunBytes: 10_737_418_240,
        maxStoreBytes: 107_374_182_400,
        maxConcurrentTransfers: 4,
        transferTimeoutMs: 3_600_000,
      },
      retention: {
        defaultMs: 2_592_000_000,
        minimumMs: 3_600_000,
        maximumMs: 31_536_000_000,
      },
      secretScanning: {
        knownSecretAction: 'reject',
        minimumSecretBytes: 8,
      },
      backend: {
        contract: 'woml.artifact-backend',
        contractVersion: 1,
        kind: 'local',
        directory: '.woml/artifacts-v1',
      },
    },
  } as const;
}

describe('Artifact Policy v1 preflight', () => {
  test('canonicalizes roots and normalizes private runtime directories', async () => {
    const { root, policy } = await fixture();
    const prepared = await preflightArtifactPolicy(policy, root, 'foreground');
    expect(prepared.projectRoot).toBe(root);
    expect(prepared.readRoots).toEqual([root, join(root, 'reports')]);
    expect(prepared.backend.kind).toBe('local');
    if (prepared.backend.kind !== 'local') throw new Error('expected local backend');
    expect(prepared.backend.directory).toBe(join(root, '.woml/artifacts-v1'));
    expect(prepared.materialization.directory).toBe(
      join(root, '.woml/materialized-v1')
    );
  });

  test('accepts only an explicit bounded operational-output rollover grant', async () => {
    const { root, policy } = await fixture();
    const prepared = await preflightArtifactPolicy({
      ...policy,
      operationalOutput: { enabled: true, retentionMs: 86_400_000 },
    }, root, 'foreground');
    expect(prepared.operationalOutput).toEqual({
      enabled: true,
      retentionMs: 86_400_000,
    });
    await expect(preflightArtifactPolicy({
      ...policy,
      operationalOutput: { enabled: false, retentionMs: 86_400_000 },
    }, root, 'foreground')).rejects.toMatchObject({
      code: 'WOML_ARTIFACT_POLICY_INVALID',
    });
  });

  test('rejects background use when policy grants foreground only', async () => {
    const { root, policy } = await fixture();
    await expect(
      preflightArtifactPolicy(policy, root, 'background')
    ).rejects.toMatchObject({ code: 'WOML_ARTIFACT_SERVICE_DISABLED' });
  });

  test('rejects read-root link escapes and unknown fields', async () => {
    if (process.platform === 'win32') return;
    const { root, policy } = await fixture();
    const outside = await mkdtemp(join(tmpdir(), 'woml-artifact-outside-'));
    temporaryDirectories.push(outside);
    await symlink(outside, join(root, 'escape'), 'dir');
    await expect(
      preflightArtifactPolicy(
        { ...policy, readRoots: ['escape'] },
        root,
        'foreground'
      )
    ).rejects.toBeInstanceOf(ArtifactPolicyError);
    await expect(
      preflightArtifactPolicy(
        { ...policy, surprise: true },
        root,
        'foreground'
      )
    ).rejects.toMatchObject({ code: 'WOML_ARTIFACT_POLICY_INVALID' });
  });

  test('normalizes an S3 backend while keeping credentials symbolic', async () => {
    const { root, policy } = await fixture();
    const prepared = await preflightArtifactPolicy(
      {
        ...policy,
        backend: {
          contract: 'woml.artifact-backend',
          contractVersion: 1,
          kind: 's3',
          bucket: 'woml-artifacts-production',
          region: 'eu-central-1',
          prefix: 'customer-a/',
          endpoint: 'https://objects.example.com',
          forcePathStyle: true,
          authentication: {
            mode: 'secrets',
            accessKeySecret: 'S3_ACCESS_KEY',
            secretKeySecret: 'S3_SECRET_KEY',
            sessionTokenSecret: 'S3_SESSION_TOKEN',
          },
          serverSideEncryption: { mode: 'aws-kms', keyId: 'alias/woml-artifacts' },
        },
      },
      root,
      'foreground'
    );
    expect(prepared.backend).toMatchObject({
      kind: 's3',
      bucket: 'woml-artifacts-production',
      prefix: 'customer-a/',
      endpoint: 'https://objects.example.com',
      authentication: {
        mode: 'secrets',
        accessKeySecret: 'S3_ACCESS_KEY',
        secretKeySecret: 'S3_SECRET_KEY',
        sessionTokenSecret: 'S3_SESSION_TOKEN',
      },
    });
    expect(
      artifactPolicySecretNames({ artifacts: { mode: 'foreground', policy: prepared } })
    ).toEqual(['S3_ACCESS_KEY', 'S3_SECRET_KEY', 'S3_SESSION_TOKEN']);
  });

  test('rejects insecure endpoints and embedded S3 credentials', async () => {
    const { root, policy } = await fixture();
    const backend = {
      contract: 'woml.artifact-backend',
      contractVersion: 1,
      kind: 's3',
      bucket: 'woml-artifacts-production',
      region: 'eu-central-1',
      prefix: '',
      endpoint: 'http://objects.example.com',
      forcePathStyle: true,
      authentication: { mode: 'ambient' },
      serverSideEncryption: { mode: 'aes256' },
    };
    await expect(
      preflightArtifactPolicy({ ...policy, backend }, root, 'foreground')
    ).rejects.toMatchObject({ code: 'WOML_ARTIFACT_POLICY_INVALID' });
    await expect(
      preflightArtifactPolicy(
        {
          ...policy,
          backend: { ...backend, endpoint: 'https://objects.example.com?credential=value' },
        },
        root,
        'foreground'
      )
    ).rejects.toMatchObject({ code: 'WOML_ARTIFACT_POLICY_INVALID' });
    await expect(
      preflightArtifactPolicy(
        {
          ...policy,
          backend: {
            ...backend,
            endpoint: 'https://objects.example.com',
            authentication: {
              mode: 'secrets',
              accessKeySecret: 'S3_ACCESS_KEY',
              secretKeySecret: 'S3_SECRET_KEY',
              accessKey: 'embedded-value-is-forbidden',
            },
          },
        },
        root,
        'foreground'
      )
    ).rejects.toMatchObject({ code: 'WOML_ARTIFACT_POLICY_INVALID' });
  });
});
