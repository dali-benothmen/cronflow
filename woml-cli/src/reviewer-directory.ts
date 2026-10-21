import { lstat, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import type {
  ReviewerDirectoryV1,
  ReviewerRuntimeAuthorityV1,
} from './rust-executor';

const MAX_REVIEWER_DIRECTORY_BYTES = 1_048_576;

export class ReviewerDirectoryLoadError extends Error {
  readonly code = 'WOML_APPROVAL_AUTHORITY_UNAVAILABLE';

  constructor(message: string) {
    super(message);
    this.name = 'ReviewerDirectoryLoadError';
  }
}

export async function loadReviewerDirectory(
  inputPath: string
): Promise<ReviewerRuntimeAuthorityV1> {
  const path = resolve(inputPath);
  let metadata;
  try {
    metadata = await lstat(path);
  } catch {
    throw new ReviewerDirectoryLoadError(
      `Reviewer directory "${path}" is not readable.`
    );
  }
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new ReviewerDirectoryLoadError(
      `Reviewer directory "${path}" must be a regular JSON file, not a symlink.`
    );
  }
  if (metadata.size < 2 || metadata.size > MAX_REVIEWER_DIRECTORY_BYTES) {
    throw new ReviewerDirectoryLoadError(
      `Reviewer directory "${path}" exceeds the supported size boundary.`
    );
  }
  let directory: unknown;
  try {
    directory = JSON.parse(await readFile(path, 'utf8'));
  } catch {
    throw new ReviewerDirectoryLoadError(
      `Reviewer directory "${path}" must contain valid JSON.`
    );
  }
  if (
    typeof directory !== 'object' ||
    directory === null ||
    Array.isArray(directory) ||
    !('profile' in directory) ||
    directory.profile !== 'woml.reviewer-directory/v1'
  ) {
    throw new ReviewerDirectoryLoadError(
      `Reviewer directory "${path}" must use profile woml.reviewer-directory/v1.`
    );
  }
  // Rust performs the authoritative closed-schema, identity, role, uniqueness,
  // and canonical-content validation before runtime activation.
  return { reviewers: { directory: directory as ReviewerDirectoryV1 } };
}
