import { afterEach, describe, expect, test } from 'bun:test';
import { mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  loadReviewerDirectory,
  ReviewerDirectoryLoadError,
} from '../src/reviewer-directory';

const directories: string[] = [];

afterEach(async () => {
  await Promise.all(
    directories
      .splice(0)
      .map(path => rm(path, { recursive: true, force: true }))
  );
});

describe('Reviewer Directory v1 loading', () => {
  test('loads the operator-owned document for authoritative Rust validation', async () => {
    const root = await mkdtemp(join(tmpdir(), 'woml-reviewers-'));
    directories.push(root);
    const path = join(root, 'reviewers.json');
    await writeFile(
      path,
      JSON.stringify({
        profile: 'woml.reviewer-directory/v1',
        version: 'directory-1',
        principals: [],
      })
    );
    const authority = await loadReviewerDirectory(path);
    expect(authority.reviewers.directory.version).toBe('directory-1');
  });

  test('rejects malformed, oversized, and symlinked authority files', async () => {
    const root = await mkdtemp(join(tmpdir(), 'woml-reviewers-invalid-'));
    directories.push(root);
    const malformed = join(root, 'malformed.json');
    await writeFile(malformed, '{');
    await expect(loadReviewerDirectory(malformed)).rejects.toBeInstanceOf(
      ReviewerDirectoryLoadError
    );

    const oversized = join(root, 'oversized.json');
    await writeFile(oversized, 'x'.repeat(1_048_577));
    await expect(loadReviewerDirectory(oversized)).rejects.toBeInstanceOf(
      ReviewerDirectoryLoadError
    );

    const valid = join(root, 'valid.json');
    const link = join(root, 'linked.json');
    await writeFile(
      valid,
      '{"profile":"woml.reviewer-directory/v1","version":"v","principals":[]}'
    );
    await symlink(valid, link);
    await expect(loadReviewerDirectory(link)).rejects.toBeInstanceOf(
      ReviewerDirectoryLoadError
    );
  });
});
