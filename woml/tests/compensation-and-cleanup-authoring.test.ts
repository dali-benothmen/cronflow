import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import Ajv2020 from 'ajv/dist/2020';

import {
  compileWoml,
  generateWomlCorrectionEditorDeclarations,
  inspectCompiledWorkflowGraph,
  parseWoml,
  WomlCompileError,
  WomlValidationError,
} from '../src';

const fixtureRoot = resolve(
  import.meta.dir,
  'fixtures/compensation-and-cleanup'
);

function fixture(name: string): string {
  return readFileSync(resolve(fixtureRoot, name), 'utf8');
}

function workflow(body: string): string {
  return `<woml><workflow id="correction-test"><triggers><manual id="start" /></triggers><steps>${body}<step id="done"><script>return true;</script></step></steps></workflow></woml>`;
}

function diagnostic(source: string): WomlValidationError | WomlCompileError {
  try {
    compileWoml(parseWoml(source, { file: 'correction-test.woml' }));
  } catch (error) {
    if (
      error instanceof WomlValidationError ||
      error instanceof WomlCompileError
    ) {
      return error;
    }
    throw error;
  }
  throw new Error('Expected correction authoring to fail.');
}

describe('WOML Compensation and Cleanup CC0 and CC1', () => {
  test('freezes Model v19 and the reviewed correction schemas', () => {
    const schemaRoot = resolve(import.meta.dir, '../../docs/schemas');
    const ajv = new Ajv2020({ allErrors: true, strict: false });
    ajv.addFormat('date-time', {
      validate: (value: string) => Number.isFinite(Date.parse(value)),
    });
    for (const name of [
      'compiled-workflow-model.v19.schema.json',
      'run-event.v18.schema.json',
      'durable-state-store.v17.schema.json',
      'script-host-protocol.v11.schema.json',
      'correction-inspection.v1.schema.json',
      'correction-recovery.v1.schema.json',
      'correction-observation.v1.schema.json',
      'compensation-and-cleanup.v1.schema.json',
    ]) {
      ajv.addSchema(JSON.parse(readFileSync(resolve(schemaRoot, name), 'utf8')));
    }

    const model = JSON.parse(fixture('model.v19.reviewed.json'));
    const validateModel = ajv.getSchema(
      'https://woml.dev/schemas/compiled-workflow-model/v19'
    );
    expect(validateModel?.(model), JSON.stringify(validateModel?.errors)).toBe(
      true
    );
    expect(
      validateModel?.({
        ...model,
        graph: {
          ...model.graph,
          sagas: [{ ...model.graph.sagas[0], bodyResultNodeId: '' }],
        },
      })
    ).toBe(false);

    expect(
      ajv
        .getSchema('https://woml.dev/schemas/durable-state-store/v17')?.(
          JSON.parse(fixture('store.v17.json'))
        )
    ).toBe(true);
    expect(
      ajv
        .getSchema('https://woml.dev/schemas/compensation-and-cleanup/v1')?.(
          JSON.parse(fixture('contracts.v1.json'))
        )
    ).toBe(true);
    const recovery = JSON.parse(fixture('recovery.v1.json'));
    const validateRecovery = ajv.getSchema(
      'https://woml.dev/schemas/correction-recovery/v1'
    );
    expect(validateRecovery?.(recovery.request)).toBe(true);
    expect(validateRecovery?.(recovery.result)).toBe(true);
    expect(
      ajv.getSchema('https://woml.dev/schemas/correction-observation/v1')?.({
        profile: 'woml.correction-observation/v1',
        openSagas: 1,
        pendingActions: 1,
        activeAttempts: 0,
        manualRecoveryActions: 1,
        oldestManualRecoveryAgeMs: 25,
      })
    ).toBe(true);
    expect(
      ajv
        .getSchema('https://woml.dev/schemas/correction-inspection/v1')?.(
          JSON.parse(fixture('inspection.v1.json'))
        )
    ).toBe(true);
    expect(
      ajv.getSchema('https://woml.dev/schemas/run-event/v18')?.({
        schemaVersion: 18,
        type: 'compensation_registered',
        runId: 'run_preview_1',
        sequence: 7,
        at: '2026-10-20T12:00:00Z',
        data: {
          sagaId: 'provision',
          actionId: 'provision:compensate:createNetwork',
          forStepId: 'createNetwork',
          completionSequence: 6,
        },
      })
    ).toBe(true);
    expect(
      ajv.getSchema('https://woml.dev/schemas/script-host-protocol/v11')?.({
        protocol: 'woml.script-host',
        protocolVersion: 11,
        messageType: 'execute',
        context: {
          compensation: {
            sagaId: 'provision',
            forStepId: 'createNetwork',
            reason: 'failure',
          },
        },
      })
    ).toBe(true);
  });

  test('deep-equals the reviewed compiled Model v19 fixture', () => {
    expect(
      compileWoml(
        parseWoml(fixture('model.reviewed.woml'), {
          file: 'model.reviewed.woml',
        })
      )
    ).toEqual(JSON.parse(fixture('model.v19.reviewed.json')));
  });

  test('lowers a DAG plus a separate corrective plan with scoped visibility', () => {
    const compiled = compileWoml(parseWoml(fixture('model.reviewed.woml')));
    if (compiled.schemaVersion !== 19) throw new Error('Expected Model v19.');
    expect(compiled.graph.sagas).toHaveLength(1);
    expect(compiled.graph.sagas[0]).toMatchObject({
      sagaId: 'provision',
      bodyEntryNodeId: 'createNetwork',
      bodyTerminalNodeId: 'deployApplication',
      bodyResultNodeId: 'deployApplication',
      outerStepIds: ['prepare'],
    });
    expect(compiled.graph.sagas[0].compensations[0]).toMatchObject({
      actionId: 'provision:compensate:createNetwork',
      forStepId: 'createNetwork',
      visibleStepIds: ['prepare', 'createNetwork'],
      node: { timeoutMs: 300_000, retryPolicy: { maxAttempts: 3 } },
    });
    expect(compiled.graph.sagas[0].cleanup[0]).toMatchObject({
      actionId: 'provision:cleanup:revokeCredentials',
      visibleStepIds: ['prepare', 'createNetwork', 'deployApplication'],
      node: { timeoutMs: 60_000, retryPolicy: { maxAttempts: 2 } },
    });
    expect(
      compiled.graph.nodes.some(node =>
        node.id.includes(':compensate:') || node.id.includes(':cleanup:')
      )
    ).toBe(false);
    expect(inspectCompiledWorkflowGraph(compiled.graph)).toEqual([]);
  });

  test('emits dynamic corrective identities for a saga inside for-each', () => {
    const source = workflow(`<step id="load"><script>return { items: [1] };</script></step><for-each id="items" items="{{context.steps.load.items}}"><saga id="provisionItem"><step id="createItem"><script>return { id: context.item };</script><compensate><script>return context.steps.createItem.id;</script></compensate></step></saga></for-each>`);
    const compiled = compileWoml(parseWoml(source));
    if (compiled.schemaVersion !== 19) throw new Error('Expected Model v19.');
    expect(compiled.graph.sagas[0].compensations[0].identity).toEqual({
      kind: 'forEachIteration',
      forEachId: 'items',
      actionIdTemplate:
        'provisionItem:compensate:createItem:{iterationKey}',
    });
  });

  test('collects compensation descriptors from selected and concurrent saga flow', () => {
    const compiled = compileWoml(
      parseWoml(fixture('control-flow.reviewed.woml'))
    );
    if (compiled.schemaVersion !== 19) throw new Error('Expected Model v19.');
    expect(
      compiled.graph.sagas[0].compensations.map(action => action.forStepId)
    ).toEqual([
      'chosenEffect',
      'unselectedEffect',
      'switchedEffect',
      'defaultEffect',
      'slowEffect',
      'fastEffect',
      'firstDestination',
      'secondDestination',
    ]);
  });

  test('rejects invalid correction shapes with source-located diagnostics', () => {
    const step = '<step id="work"><script>return true;</script></step>';
    const cases = [
      [
        workflow('<step id="work"><script>return true;</script><compensate><script>return true;</script></compensate></step>'),
        'WOML_COMPENSATE_OUTSIDE_SAGA',
      ],
      [
        workflow(`<saga id="outer"><saga id="inner">${step}</saga></saga>`),
        'WOML_SAGA_NESTED',
      ],
      [
        workflow(`<saga id="scope">${step}<cleanup>${step}</cleanup>${step}</saga>`),
        'WOML_CLEANUP_ORDER_INVALID',
      ],
      [
        workflow(`<saga id="scope">${step}<cleanup><wait id="later" duration="1m" /></cleanup></saga>`),
        'WOML_CLEANUP_BODY_INVALID',
      ],
      [
        workflow(`<saga id="scope"><step id="work"><script>return true;</script><compensate><wait id="later" duration="1m" /></compensate></step></saga>`),
        'WOML_COMPENSATE_BODY_INVALID',
      ],
      [
        workflow(`<saga id="scope"><fork id="routes" join="joined"><branch id="joined">${step}</branch><branch id="detached"><step id="audit"><script>return true;</script></step></branch></fork></saga>`),
        'WOML_SAGA_DETACHED_BRANCH',
      ],
    ] as const;
    for (const [source, code] of cases) {
      const error = diagnostic(source);
      expect(error.diagnostic.code).toBe(code);
      expect(error.diagnostic.file).toBe('correction-test.woml');
      expect(error.diagnostic.location.start.line).toBeGreaterThan(0);
    }
  });

  test('exposes correction editor declarations', () => {
    const declarations = generateWomlCorrectionEditorDeclarations();
    expect(declarations).toContain('readonly compensation?');
    expect(declarations).toContain('readonly cleanup?');
    expect(declarations).toContain("readonly reason: 'failure' | 'cancel'");
  });

  test('rejects a divergent saga descriptor during structural inspection', () => {
    const compiled = compileWoml(parseWoml(fixture('model.reviewed.woml')));
    if (compiled.schemaVersion !== 19) throw new Error('Expected Model v19.');
    expect(
      inspectCompiledWorkflowGraph({
        ...compiled.graph,
        sagas: [{ ...compiled.graph.sagas[0], bodyResultNodeId: 'missing' }],
      })
    ).toContainEqual(expect.objectContaining({ code: 'INVALID_SAGA' }));
  });
});
