import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import Ajv2020 from 'ajv/dist/2020';

import {
  buildWomlExecutableDefinitionPackage,
  compileWoml,
  inspectCompiledWorkflowGraph,
  parseWoml,
  WomlCompileError,
  WomlValidationError,
} from '../src';

const fixtureRoot = resolve(import.meta.dir, 'fixtures/rich-plan-approvals');
const schemaRoot = resolve(import.meta.dir, '../../docs/schemas');
const fixture = (name: string): string =>
  readFileSync(resolve(fixtureRoot, name), 'utf8');

function workflow(items: string): string {
  return `<woml><workflow id="review-test"><triggers><manual id="start" /></triggers><steps>${items}</steps></workflow></woml>`;
}

const buildReview =
  '<step id="buildReview"><script>return { review: { title: "Change", summary: "Reviewed change", risk: "low" } };</script></step>';
const arms =
  '<when-approved><step id="accepted"><script>return true;</script></step></when-approved><when-rejected><step id="declined"><script>return false;</script></step></when-rejected>';

function diagnostic(source: string): WomlValidationError | WomlCompileError {
  try {
    compileWoml(parseWoml(source, { file: 'rich-approval-test.woml' }));
  } catch (error) {
    if (
      error instanceof WomlValidationError ||
      error instanceof WomlCompileError
    ) {
      return error;
    }
    throw error;
  }
  throw new Error('Expected rich approval authoring to fail.');
}

describe('WOML Rich Plan Approvals RPA0 and RPA1', () => {
  test('freezes every shared v1/v2/v3 contract with fail-closed schemas', () => {
    const ajv = new Ajv2020({ allErrors: true, strict: false });
    ajv.addFormat('date-time', {
      validate: (value: string) => Number.isFinite(Date.parse(value)),
    });
    ajv.addFormat('uri', {
      validate: (value: string) => /^https:\/\//.test(value),
    });
    for (const name of [
      'artifact-reference.v1.schema.json',
      'review-payload.v1.schema.json',
      'reviewer-directory.v1.schema.json',
      'rich-plan-approvals.v1.schema.json',
      'compiled-workflow-model.v20.schema.json',
      'run-event.v19.schema.json',
      'durable-state-store.v18.schema.json',
      'approval-http.v2.schema.json',
      'notification-provider-host.v3.schema.json',
      'approval-inspection.v2.schema.json',
    ]) {
      ajv.addSchema(JSON.parse(readFileSync(resolve(schemaRoot, name), 'utf8')));
    }

    const cases = [
      ['https://woml.dev/schemas/rich-plan-approvals/v1', 'contracts.v1.json'],
      ['https://woml.dev/schemas/review-payload/v1', 'review.v1.json'],
      ['https://woml.dev/schemas/reviewer-directory/v1', 'reviewer-directory.v1.json'],
      ['https://woml.dev/schemas/compiled-workflow-model/v20', 'model.v20.reviewed.json'],
      ['https://woml.dev/schemas/durable-state-store/v18', 'store.v18.json'],
      ['https://woml.dev/schemas/approval-http/v2', 'http.v2.json'],
      ['https://woml.dev/schemas/notification-provider-host/v3', 'provider.v3.json'],
      ['https://woml.dev/schemas/approval-inspection/v2', 'inspection.v2.json'],
    ] as const;
    for (const [schema, name] of cases) {
      const validate = ajv.getSchema(schema)!;
      const value = JSON.parse(fixture(name));
      expect(validate(value), `${name}: ${JSON.stringify(validate.errors)}`).toBe(
        true
      );
      expect(validate({ ...value, unexpected: true })).toBe(false);
    }
    const validateEvent = ajv.getSchema(
      'https://woml.dev/schemas/run-event/v19'
    )!;
    for (const event of JSON.parse(fixture('events.v19.json'))) {
      expect(validateEvent(event), JSON.stringify(validateEvent.errors)).toBe(
        true
      );
    }
  });

  test('deep-equals the reviewed Model v20 golden fixture', () => {
    const compiled = compileWoml(
      parseWoml(fixture('model.reviewed.woml'), {
        file: 'model.reviewed.woml',
      })
    );
    expect(compiled).toEqual(JSON.parse(fixture('model.v20.reviewed.json')));
    expect(inspectCompiledWorkflowGraph(compiled.graph)).toEqual([]);
  });

  test('lowers a sorted deterministic rich approval descriptor', () => {
    const compiled = compileWoml(parseWoml(fixture('model.reviewed.woml')));
    expect(compiled.schemaVersion).toBe(20);
    const wait = compiled.graph.nodes.find(
      node => node.id === 'approveDeployment'
    )!;
    expect(wait.inputs).toEqual(
      expect.objectContaining({
        kind: 'object',
        fields: expect.objectContaining({
          review: {
            kind: 'contextReference',
            path: ['steps', 'buildReview', 'review'],
          },
          requiredApprovals: { kind: 'literal', value: 2 },
          requiredRoles: {
            kind: 'array',
            items: [
              { kind: 'literal', value: 'production-owner' },
              { kind: 'literal', value: 'security-owner' },
            ],
          },
        }),
      })
    );
  });

  test('keeps basic approvals on their original compiled model', () => {
    const source = workflow(
      `<approval id="review">${arms}</approval><step id="done"><script>return context.steps.review;</script></step>`
    );
    const first = compileWoml(parseWoml(source));
    const second = compileWoml(parseWoml(source));
    expect(first.schemaVersion).toBe(4);
    expect(JSON.stringify(first)).toBe(JSON.stringify(second));
    const wait = first.graph.nodes.find(node => node.id === 'review')!;
    expect(Object.keys(wait.inputs.kind === 'object' ? wait.inputs.fields : {})).toEqual([
      'onTimeout',
    ]);
  });

  test('packages module-backed rich approvals deterministically as Definition Package v15', async () => {
    const path = resolve(fixtureRoot, 'model-module.reviewed.woml');
    const document = parseWoml(readFileSync(path, 'utf8'), { file: path });
    const first = await buildWomlExecutableDefinitionPackage(document, {
      sourcePath: path,
      projectRoot: fixtureRoot,
    });
    const second = await buildWomlExecutableDefinitionPackage(document, {
      sourcePath: path,
      projectRoot: fixtureRoot,
    });
    expect(first.schemaVersion).toBe(15);
    expect(first.profile).toBe('woml.definition-package/v15');
    expect(first.workflow.model.schemaVersion).toBe(20);
    expect(first.rootHash).toBe(second.rootHash);
  });

  test('rejects invalid policy attributes with source-located diagnostics', () => {
    const cases = [
      [workflow(`<approval id="review" approvals="2">${arms}</approval>`), 'WOML_APPROVAL_REVIEW_REQUIRED'],
      [workflow(`${buildReview}<approval id="review" review="{{context.payload.review}}">${arms}</approval>`), 'WOML_APPROVAL_REVIEW_REFERENCE_INVALID'],
      [workflow(`${buildReview}<approval id="review" review="{{secrets.PLAN}}">${arms}</approval>`), 'WOML_SECRET_SINK_UNSUPPORTED'],
      [workflow(`${buildReview}<approval id="review" review="{{context.steps.buildReview[dynamic]}}">${arms}</approval>`), 'WOML_APPROVAL_REVIEW_REFERENCE_INVALID'],
      [workflow(`${buildReview}<approval id="review" review="{{context.steps.buildReview.review}}" approvals="0">${arms}</approval>`), 'WOML_APPROVAL_COUNT_INVALID'],
      [workflow(`${buildReview}<approval id="review" review="{{context.steps.buildReview.review}}" approvals="21">${arms}</approval>`), 'WOML_APPROVAL_COUNT_INVALID'],
      [workflow(`${buildReview}<approval id="review" review="{{context.steps.buildReview.review}}" required-roles="owner owner">${arms}</approval>`), 'WOML_APPROVAL_ROLE_DUPLICATE'],
      [workflow(`${buildReview}<approval id="review" review="{{context.steps.buildReview.review}}" required-roles="ProductionOwner">${arms}</approval>`), 'WOML_APPROVAL_ROLE_INVALID'],
    ] as const;
    for (const [source, code] of cases) {
      const error = diagnostic(source);
      expect(error.diagnostic.code).toBe(code);
      expect(error.diagnostic.file).toBe('rich-approval-test.woml');
      expect(error.diagnostic.location.start.line).toBeGreaterThan(0);
    }
  });

  test('rejects unknown and forward review evidence', () => {
    const unknown = workflow(
      `<approval id="review" review="{{context.steps.missing.review}}">${arms}</approval>`
    );
    expect(diagnostic(unknown).diagnostic.code).toBe('WOML_UNKNOWN_REFERENCE');

    const forward = workflow(
      `<approval id="review" review="{{context.steps.buildReview.review}}">${arms}</approval>${buildReview}`
    );
    expect(diagnostic(forward).diagnostic.code).toBe(
      'WOML_REFERENCE_NOT_DOMINATING'
    );
  });

  test('rejects partial or malformed compiled rich descriptors', () => {
    const compiled = compileWoml(parseWoml(fixture('model.reviewed.woml')));
    const nodes = compiled.graph.nodes.map(node =>
      node.id !== 'approveDeployment' || node.inputs.kind !== 'object'
        ? node
        : {
            ...node,
            inputs: {
              ...node.inputs,
              fields: {
                ...node.inputs.fields,
                requiredApprovals: { kind: 'literal' as const, value: 0 },
              },
            },
          }
    );
    expect(
      inspectCompiledWorkflowGraph({ ...compiled.graph, nodes })
    ).toContainEqual(expect.objectContaining({ code: 'INVALID_APPROVAL_GROUP' }));
  });
});
