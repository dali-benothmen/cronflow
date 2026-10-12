import { describe, expect, test } from 'bun:test';
import { resolve } from 'node:path';

import Ajv2020, { type ValidateFunction } from 'ajv/dist/2020';

type JsonObject = Record<string, any>;

const repositoryRoot = resolve(import.meta.dir, '../..');
const schemaRoot = resolve(repositoryRoot, 'docs/schemas');
const fixtureRoot = resolve(import.meta.dir, 'fixtures/durable-artifacts');

async function json(path: string): Promise<any> {
  return Bun.file(path).json();
}

async function contractValidators() {
  const ajv = new Ajv2020({
    allErrors: true,
    strict: false,
  });
  ajv.addFormat('date-time', {
    validate: (value: string) => Number.isFinite(Date.parse(value)),
  });
  ajv.addFormat('uri', {
    validate: (value: string) => {
      try {
        const url = new URL(value);
        return url.protocol === 'https:' || url.protocol === 'http:';
      } catch {
        return false;
      }
    },
  });
  for (const name of [
    'capability-call.v1.schema.json',
    'artifact-reference.v1.schema.json',
    'artifact-backend.v1.schema.json',
    'artifact-policy.v1.schema.json',
    'artifacts.v1.schema.json',
    'artifact-operation-metadata.v1.schema.json',
    'artifact-operation-settlement.v1.schema.json',
    'artifact-maintenance.v1.schema.json',
    'artifact-inspection.v1.schema.json',
    'artifact-run-inspection.v1.schema.json',
    'run-inspection.v7.schema.json',
    'artifact-export.v1.schema.json',
    'artifact-observation.v1.schema.json',
    'runtime-metrics.v2.schema.json',
  ]) {
    ajv.addSchema(await json(resolve(schemaRoot, name)));
  }
  const get = (id: string): ValidateFunction => {
    const validate = ajv.getSchema(id);
    if (validate === undefined) throw new Error(`Missing schema ${id}`);
    return validate;
  };
  return {
    reference: get('https://woml.dev/schemas/artifact-reference/v1'),
    artifacts: get('https://woml.dev/schemas/artifacts/v1'),
    policy: get('https://woml.dev/schemas/artifact-policy/v1'),
    backend: get('https://woml.dev/schemas/artifact-backend/v1'),
    metadata: get('https://woml.dev/schemas/artifact-operation-metadata/v1'),
    settlement: get('https://woml.dev/schemas/artifact-operation-settlement/v1'),
    maintenance: get('https://woml.dev/schemas/artifact-maintenance/v1'),
    inspection: get('https://woml.dev/schemas/artifact-inspection/v1'),
    runInspection: get('https://woml.dev/schemas/artifact-run-inspection/v1'),
    compositeRunInspection: get('https://woml.dev/schemas/run-inspection/v7'),
    export: get('https://woml.dev/schemas/artifact-export/v1'),
    observation: get('https://woml.dev/schemas/artifact-observation/v1'),
    metricV2: get('https://woml.dev/schemas/runtime-metrics/v2'),
    capability: get('https://cronflow.dev/schemas/capability-call/v1'),
    capabilityFailure: get(
      'https://cronflow.dev/schemas/capability-call/v1#/$defs/failure'
    ),
  };
}

describe('Frozen Durable Artifacts v1 contracts', () => {
  test('validates the reviewed reference, requests, results, policy, and both backends', async () => {
    const validators = await contractValidators();
    const contracts = (await json(
      resolve(fixtureRoot, 'contracts.v1.json')
    )) as JsonObject;

    expect(
      validators.reference(contracts.reference),
      JSON.stringify(validators.reference.errors)
    ).toBe(true);
    for (const value of [
      ...Object.values(contracts.requests),
      ...Object.values(contracts.results),
    ]) {
      expect(
        validators.artifacts(value),
        JSON.stringify(validators.artifacts.errors)
      ).toBe(true);
    }

    const policy = (await json(resolve(fixtureRoot, 'policy.v1.json'))) as JsonObject;
    expect(validators.policy(policy), JSON.stringify(validators.policy.errors)).toBe(
      true
    );
    expect(policy.retention.minimumMs).toBeLessThanOrEqual(
      policy.retention.defaultMs
    );
    expect(policy.retention.defaultMs).toBeLessThanOrEqual(
      policy.retention.maximumMs
    );
    expect(policy.limits.maxArtifactBytes).toBeLessThanOrEqual(
      policy.limits.maxRunBytes
    );
    expect(policy.limits.maxRunBytes).toBeLessThanOrEqual(
      policy.limits.maxStoreBytes
    );

    const backends = (await json(
      resolve(fixtureRoot, 'backends.v1.json')
    )) as JsonObject[];
    expect(backends.map(backend => backend.kind)).toEqual(['local', 's3']);
    for (const backend of backends) {
      expect(
        validators.backend(backend),
        JSON.stringify(validators.backend.errors)
      ).toBe(true);
    }
  });

  test('carries Artifact v1 through unchanged Capability Call v1 envelopes', async () => {
    const validators = await contractValidators();
    const fixture = (await json(
      resolve(fixtureRoot, 'capability-put.v1.json')
    )) as JsonObject;

    expect(
      validators.capability(fixture.request),
      JSON.stringify(validators.capability.errors)
    ).toBe(true);
    expect(
      validators.artifacts(fixture.request.input),
      JSON.stringify(validators.artifacts.errors)
    ).toBe(true);
    expect(fixture.request).toMatchObject({
      capability: 'artifacts',
      operation: 'put',
      inputContractVersion: 1,
      resultContractVersion: 1,
      identity: {
        mode: 'named',
        operationName: fixture.request.input.input.name,
      },
    });

    expect(
      validators.capability(fixture.result),
      JSON.stringify(validators.capability.errors)
    ).toBe(true);
    expect(
      validators.artifacts(fixture.result.result),
      JSON.stringify(validators.artifacts.errors)
    ).toBe(true);
  });

  test('validates body-free operation metadata and durable settlement proofs', async () => {
    const validators = await contractValidators();
    const metadata = (await json(
      resolve(fixtureRoot, 'operation-metadata.v1.json')
    )) as JsonObject[];
    const settlements = (await json(
      resolve(fixtureRoot, 'settlements.v1.json')
    )) as JsonObject[];

    for (const value of metadata) {
      expect(
        validators.metadata(value),
        JSON.stringify(validators.metadata.errors)
      ).toBe(true);
    }
    for (const value of settlements) {
      expect(
        validators.settlement(value),
        JSON.stringify(validators.settlement.errors)
      ).toBe(true);
    }
    expect(settlements.map(value => value.status)).toEqual([
      'started',
      'succeeded',
      'interrupted',
    ]);
  });

  test('freezes body-free artifact retention and integrity measurements', async () => {
    const validators = await contractValidators();
    const reports = (await json(
      resolve(fixtureRoot, 'maintenance.v1.json')
    )) as JsonObject[];
    for (const report of reports) {
      expect(
        validators.maintenance(report),
        JSON.stringify(validators.maintenance.errors)
      ).toBe(true);
      const encoded = JSON.stringify(report);
      for (const forbidden of ['name', 'path', 'contentType', 'body', 'credential']) {
        expect(encoded).not.toContain(forbidden);
      }
    }
  });

  test('freezes bounded operator inspection, audited export, and body-free telemetry', async () => {
    const validators = await contractValidators();
    const fixture = (await json(resolve(fixtureRoot, 'operator.v1.json'))) as JsonObject;
    for (const [name, validator] of [
      ['inspection', validators.inspection],
      ['runInspection', validators.runInspection],
      ['compositeRunInspection', validators.compositeRunInspection],
      ['export', validators.export],
      ['observation', validators.observation],
      ['metric', validators.metricV2],
    ] as const) {
      expect(validator(fixture[name]), JSON.stringify(validator.errors)).toBe(true);
    }
    const encoded = JSON.stringify(fixture);
    for (const forbidden of [
      'signedUrl', 'credentials', 'sourcePath', 'materializedPath', 'body',
      'secret-value',
    ]) {
      expect(encoded).not.toContain(forbidden);
    }
  });

  test('freezes every author-facing failure code and ambiguity classification', async () => {
    const validators = await contractValidators();
    const failures = (await json(
      resolve(fixtureRoot, 'failures.v1.json')
    )) as JsonObject[];

    expect(failures.map(failure => failure.code)).toEqual([
      'WOML_ARTIFACT_SERVICE_DISABLED',
      'WOML_ARTIFACT_PATH_INVALID',
      'WOML_ARTIFACT_SOURCE_DENIED',
      'WOML_ARTIFACT_SOURCE_CHANGED',
      'WOML_ARTIFACT_SOURCE_TYPE_UNSUPPORTED',
      'WOML_ARTIFACT_TOO_LARGE',
      'WOML_ARTIFACT_QUOTA_EXCEEDED',
      'WOML_ARTIFACT_RETENTION_INVALID',
      'WOML_ARTIFACT_NOT_FOUND',
      'WOML_ARTIFACT_ACCESS_DENIED',
      'WOML_ARTIFACT_EXPIRED',
      'WOML_ARTIFACT_QUARANTINED',
      'WOML_ARTIFACT_CORRUPT',
      'WOML_ARTIFACT_BACKEND_UNAVAILABLE',
      'WOML_ARTIFACT_TIMED_OUT',
      'WOML_ARTIFACT_CANCELLED',
      'WOML_ARTIFACT_MATERIALIZATION_UNAVAILABLE',
      'WOML_ARTIFACT_OPERATION_IDENTITY_CONFLICT',
      'WOML_ARTIFACT_INTERRUPTED',
    ]);
    for (const failure of failures) {
      expect(
        validators.capabilityFailure(failure),
        `${failure.code}: ${JSON.stringify(validators.capabilityFailure.errors)}`
      ).toBe(true);
    }
    expect(
      failures.find(failure => failure.code === 'WOML_ARTIFACT_INTERRUPTED')
    ).toMatchObject({ retryable: false, ambiguous: true });
  });

  test('rejects unsafe paths, future versions, leaked metadata, and embedded credentials', async () => {
    const validators = await contractValidators();
    const rejections = (await json(
      resolve(fixtureRoot, 'rejections.v1.json')
    )) as { readonly schema: keyof typeof validators; readonly value: unknown }[];

    for (const rejection of rejections) {
      const validate = validators[rejection.schema] as ValidateFunction;
      expect(validate(rejection.value), JSON.stringify(rejection)).toBe(false);
    }

    const contracts = (await json(
      resolve(fixtureRoot, 'contracts.v1.json')
    )) as JsonObject;
    const oversizedName = {
      ...contracts.reference,
      name: 'x'.repeat(257),
    };
    expect(validators.reference(oversizedName)).toBe(false);
    const nulPath = structuredClone(contracts.requests.put);
    nulPath.input.sourcePath = 'reports/secret\u0000.txt';
    expect(validators.artifacts(nulPath)).toBe(false);
    const futureRequest = structuredClone(contracts.requests.put);
    futureRequest.contractVersion = 2;
    expect(validators.artifacts(futureRequest)).toBe(false);
  });

  test('pins the accepted, rejected, and deferred product behavior matrix', async () => {
    const matrix = (await json(
      resolve(fixtureRoot, 'behavior-matrix.v1.json')
    )) as JsonObject;
    expect(matrix.accepted).toHaveLength(8);
    expect(matrix.rejected).toHaveLength(9);
    expect(matrix.deferred).toEqual([
      'workflow-call parent or child grants',
      'public or signed download URLs',
      'artifact-bound approval decisions',
      'remote-execution artifact transport',
      'arbitrary format parsing',
      'transparent local encryption at rest',
    ]);
  });

  test('keeps the reviewed WOML journey and DA0 security decisions visible', async () => {
    const workflow = await Bun.file(resolve(fixtureRoot, 'reviewed.woml')).text();
    const contract = await Bun.file(
      resolve(repositoryRoot, 'docs/protocols/durable-artifacts-v1.md')
    ).text();
    const threatModel = await Bun.file(
      resolve(repositoryRoot, 'docs/durable-artifacts-v1-threat-model.md')
    ).text();

    for (const marker of [
      'services.artifacts.put',
      'services.artifacts.materialize',
      'context.steps.createPlan.plan',
    ]) {
      expect(workflow).toContain(marker);
    }
    for (const marker of [
      'same-run in v1',
      'Artifact ID, body digest, and backend object key are distinct identities',
      'Workflow code cannot delete an artifact',
      'Capability Call v1',
    ]) {
      expect(contract).toContain(marker);
    }
    for (const marker of [
      'Symlink, junction, or reparse-point escape',
      'Cross-run reference forgery',
      'Materialized file outlives attempt',
    ]) {
      expect(threatModel).toContain(marker);
    }
  });
});
