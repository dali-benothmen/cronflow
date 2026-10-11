#!/usr/bin/env bun

import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { womlNativeTargets } from '../src/native-platform';

const root = resolve(import.meta.dir, '../..');
const read = (path: string): Promise<string> =>
  readFile(resolve(root, path), 'utf8');

const [ci, release, smoke, manifest] = await Promise.all([
  read('.github/workflows/ci.yml'),
  read('.github/workflows/release.yml'),
  read('woml-cli/scripts/smoke-release-candidate.ts'),
  read('woml-cli/package.json'),
]);

for (const marker of [
  'Process containment (${{ matrix.os }})',
  'ubuntu-24.04',
  'macos-15',
  'windows-2022',
  'process::tests',
  '--test-threads=1',
]) {
  if (!ci.includes(marker)) {
    throw new Error(`Process v1 CI containment gate is missing ${marker}.`);
  }
}

for (const target of womlNativeTargets) {
  if (!release.includes(`package: ${target}`)) {
    throw new Error(
      `Process v1 packaged smoke is missing native target ${target}.`
    );
  }
}
for (const marker of [
  'smoke-release-candidate.ts',
  'PROCESS_CONTAINMENT_SMOKE_V1',
  'process-literal.woml',
  'process-tree.woml',
  'WOML_PROCESS_TIMED_OUT',
  'descendant survived timeout containment',
]) {
  if (!release.includes(marker) && !smoke.includes(marker)) {
    throw new Error(
      `Process v1 packaged-release evidence is missing ${marker}.`
    );
  }
}

for (const path of [
  'docs/woml-process.md',
  'examples/process/project-checks.woml',
  'examples/process/kubernetes-rollout-check.woml',
  'examples/process/release-verification.woml',
  'examples/process/process-policy.example.json',
  'skills/woml/references/services.md',
]) {
  const source = await read(path);
  if (source.trim().length === 0) {
    throw new Error(`Process v1 release artifact is empty: ${path}.`);
  }
}

for (const path of [
  'examples/process/project-checks.woml',
  'examples/process/kubernetes-rollout-check.woml',
  'examples/process/release-verification.woml',
]) {
  const source = await read(path);
  if (
    !source.includes('services.process.run(') ||
    source.includes('Bun.spawn(')
  ) {
    throw new Error(`${path} must use the supervised Process v1 API.`);
  }
}

if (!manifest.includes('dist/script-host-worker.js')) {
  throw new Error(
    'The portable package omits the Process v1 script-host facade.'
  );
}

process.stdout.write(
  `[process-release] contracts, documentation, examples, CI containment, and ${womlNativeTargets.length} packaged native targets are present\n`
);
