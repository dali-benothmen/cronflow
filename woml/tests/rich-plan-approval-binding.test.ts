import { describe, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

function canonicalJson(value: JsonValue): string {
  if (value === null || typeof value !== 'object') {
    const encoded = JSON.stringify(value);
    if (encoded === undefined) throw new Error('Non-JSON value in binding vector.');
    return encoded;
  }
  if (Array.isArray(value)) {
    return `[${value.map(canonicalJson).join(',')}]`;
  }
  return `{${Object.keys(value)
    .sort()
    .map(key => `${JSON.stringify(key)}:${canonicalJson(value[key]!)}`)
    .join(',')}}`;
}

describe('Rich Plan Approval canonical binding', () => {
  test('matches the frozen RFC 8785 digest produced by Rust', () => {
    const vector = JSON.parse(
      readFileSync(
        resolve(
          import.meta.dir,
          'fixtures/rich-plan-approvals/binding.v1.json'
        ),
        'utf8'
      )
    ) as { binding: JsonValue; digest: string };
    const digest = `sha256:${createHash('sha256')
      .update(canonicalJson(vector.binding))
      .digest('hex')}`;
    expect(digest).toBe(vector.digest);
  });
});
