import { describe, expect, test } from 'bun:test';
import { resolve } from 'node:path';

import Ajv2020, { type ValidateFunction } from 'ajv/dist/2020';

const repositoryRoot = resolve(import.meta.dir, '../..');
const schemaRoot = resolve(repositoryRoot, 'docs/schemas');
const fixtureRoot = resolve(import.meta.dir, 'fixtures/streaming-output');

async function readJson(path: string): Promise<any> {
  return Bun.file(path).json();
}

async function validators() {
  const ajv = new Ajv2020({ allErrors: true, strict: false });
  ajv.addFormat('date-time', {
    validate: (value: string) => Number.isFinite(Date.parse(value)),
  });
  for (const name of [
    'artifact-reference.v1.schema.json',
    'operational-record.v1.schema.json',
    'operational-page.v1.schema.json',
    'operational-stream.v1.schema.json',
    'operational-output-store.v1.schema.json',
    'process.v1.schema.json',
    'process.v2.schema.json',
    'script-host-protocol.v12.schema.json',
  ]) {
    ajv.addSchema(await readJson(resolve(schemaRoot, name)));
  }
  const get = (id: string): ValidateFunction => {
    const validator = ajv.getSchema(id);
    if (validator === undefined) throw new Error(`missing schema ${id}`);
    return validator;
  };
  return {
    record: get('https://woml.dev/schemas/operational-record/v1'),
    page: get('https://woml.dev/schemas/operational-page/v1'),
    stream: get('https://woml.dev/schemas/operational-stream/v1'),
    store: get('https://woml.dev/schemas/operational-output-store/v1'),
    processV1: get('https://woml.dev/schemas/process/v1'),
    processV2: get('https://woml.dev/schemas/process/v2'),
    scriptV12: get('https://woml.dev/schemas/script-host-protocol/v12'),
  };
}

describe('Streaming Output and Progress v1 contracts', () => {
  test('validates reviewed records, pages, streams, store inspection, and producer extensions', async () => {
    const validate = await validators();
    const fixture = await readJson(resolve(fixtureRoot, 'contracts.v1.json'));
    for (const record of [fixture.record, fixture.progressRecord]) {
      expect(validate.record(record), JSON.stringify(validate.record.errors)).toBe(
        true
      );
      expect(JSON.parse(JSON.stringify(record))).toEqual(record);
    }
    expect(validate.page(fixture.page), JSON.stringify(validate.page.errors)).toBe(
      true
    );
    expect(
      validate.stream(fixture.stream),
      JSON.stringify(validate.stream.errors)
    ).toBe(true);
    expect(
      validate.store(fixture.storeInspection),
      JSON.stringify(validate.store.errors)
    ).toBe(true);
    expect(
      validate.processV2(fixture.processV2),
      JSON.stringify(validate.processV2.errors)
    ).toBe(true);
    expect(
      validate.processV2(fixture.processV2Result),
      JSON.stringify(validate.processV2.errors)
    ).toBe(true);
    for (const frame of fixture.scriptFrames) {
      expect(
        validate.scriptV12(frame),
        JSON.stringify(validate.scriptV12.errors)
      ).toBe(true);
    }
  });

  test('keeps Process v1 valid and separate from opt-in Process v2 streaming', async () => {
    const validate = await validators();
    const processV1 = await readJson(
      resolve(
        import.meta.dir,
        'fixtures/services-contracts/process-request.v1.json'
      )
    );
    expect(
      validate.processV1(processV1),
      JSON.stringify(validate.processV1.errors)
    ).toBe(true);
    expect(validate.processV2(processV1)).toBe(false);
  });

  test('rejects unknown fields, future profiles, oversized bodies, and worker-forged identity', async () => {
    const validate = await validators();
    const fixture = await readJson(resolve(fixtureRoot, 'contracts.v1.json'));

    expect(validate.record({ ...fixture.record, secret: 'must-not-pass' })).toBe(
      false
    );
    expect(validate.record({ ...fixture.record, profile: 'woml.operational-record/v2' })).toBe(
      false
    );
    expect(
      validate.record({
        ...fixture.record,
        text: 'x'.repeat(16_385),
      })
    ).toBe(false);
    expect(
      validate.scriptV12({
        ...fixture.scriptFrames[0],
        runId: 'forged-run',
        nodeId: 'forged-step',
      })
    ).toBe(false);
    expect(
      validate.processV2({
        ...fixture.processV2,
        contractVersion: 3,
      })
    ).toBe(false);
  });

  test('freezes every reviewed resource budget and output-path owner', async () => {
    const budgets = await readJson(resolve(fixtureRoot, 'budgets.v1.json'));
    expect(budgets).toMatchObject({
      decodedChunkBytes: 16_384,
      scriptFrameBytes: 4_096,
      pageRecords: 1_000,
      pageBytes: 1_048_576,
      followersPerRuntime: 8,
      defaultInlineRunBytes: 16_777_216,
      maximumInlineRunBytes: 268_435_456,
      defaultRetentionMs: 604_800_000,
      defaultStoreBytes: 1_073_741_824,
      progressSnapshotsPerSecondPerLane: 4,
      napiDelivery: 'coalesced-cursor-wakeup',
    });

    const inventory = await readJson(resolve(fixtureRoot, 'inventory.v1.json'));
    const names = new Set<string>();
    for (const path of inventory.paths) {
      expect(path.owner.length).toBeGreaterThan(0);
      expect(names.has(path.name)).toBe(false);
      names.add(path.name);
    }
    expect(names.size).toBeGreaterThanOrEqual(15);

    const threatModel = await readJson(
      resolve(fixtureRoot, 'threat-model.v1.json')
    );
    expect(threatModel.invariants.length).toBeGreaterThanOrEqual(12);
    expect(threatModel.untrustedSources).toContain('ai-output');
  });
});
