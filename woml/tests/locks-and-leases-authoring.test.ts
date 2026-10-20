import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import Ajv2020 from 'ajv/dist/2020';

import {
  buildWomlReusableDefinitionPackage,
  compileWoml,
  compileWomlWithModules,
  generateWomlEditorDeclarations,
  generateWomlLockEditorDeclarations,
  inspectCompiledWorkflowGraph,
  parseWoml,
  resolveWomlReusableDefinitionGraph,
  WomlCompileError,
  WomlValidationError,
} from '../src';

const fixtureRoot = resolve(import.meta.dir, 'fixtures/locks-and-leases');

function fixture(name: string): string {
  return readFileSync(resolve(fixtureRoot, name), 'utf8');
}

function workflow(body: string, before = ''): string {
  return `<woml><workflow id="lock-test"><triggers><manual id="start" /></triggers><steps>${before}${body}<step id="done"><script>return true;</script></step></steps></workflow></woml>`;
}

function diagnostic(source: string): WomlValidationError | WomlCompileError {
  try {
    compileWoml(parseWoml(source, { file: 'lock-test.woml' }));
  } catch (error) {
    if (
      error instanceof WomlValidationError ||
      error instanceof WomlCompileError
    ) {
      return error;
    }
    throw error;
  }
  throw new Error('Expected lock authoring to fail.');
}

describe('WOML Locks and Leases LL0 and LL1', () => {
  test('freezes Model v18 and the reviewed lock contract schemas', () => {
    const schemaRoot = resolve(import.meta.dir, '../../docs/schemas');
    const ajv = new Ajv2020({ allErrors: true, strict: false });
    ajv.addFormat('date-time', {
      validate: (value: string) => Number.isFinite(Date.parse(value)),
    });
    for (const name of [
      'compiled-workflow-model.v18.schema.json',
      'run-event.v17.schema.json',
      'durable-state-store.v16.schema.json',
      'lock-inspection.v1.schema.json',
      'locks-capability.v1.schema.json',
      'locks-and-leases.v1.schema.json',
    ]) {
      ajv.addSchema(JSON.parse(readFileSync(resolve(schemaRoot, name), 'utf8')));
    }

    const model = JSON.parse(fixture('model.v18.reviewed.json'));
    const modelValidator = ajv.getSchema(
      'https://woml.dev/schemas/compiled-workflow-model/v18'
    );
    expect(modelValidator?.(model), JSON.stringify(modelValidator?.errors)).toBe(
      true
    );
    expect(
      modelValidator?.({
        ...model,
        graph: { ...model.graph, locks: [{ ...model.graph.locks[0], leaseMs: 1 }] },
      })
    ).toBe(false);

    const storeValidator = ajv.getSchema(
      'https://woml.dev/schemas/durable-state-store/v16'
    );
    expect(storeValidator?.(JSON.parse(fixture('store.v16.json')))).toBe(true);
    const contractValidator = ajv.getSchema(
      'https://woml.dev/schemas/locks-and-leases/v1'
    );
    expect(contractValidator?.(JSON.parse(fixture('contracts.v1.json')))).toBe(
      true
    );

    const eventValidator = ajv.getSchema(
      'https://woml.dev/schemas/run-event/v17'
    );
    expect(
      eventValidator?.({
        schemaVersion: 17,
        type: 'lock_acquired',
        runId: 'run_1',
        sequence: 4,
        at: '2026-10-20T10:00:00Z',
        data: {
          lockId: 'deployment',
          key: 'production/payments',
          fencingToken: '42',
        },
      })
    ).toBe(true);
    expect(
      eventValidator?.({
        schemaVersion: 17,
        type: 'lock_acquired',
        runId: 'run_1',
        sequence: 4,
        at: '2026-10-20T10:00:00Z',
        data: { lockId: 'deployment', key: 'production/payments', fencingToken: 42 },
      })
    ).toBe(false);
  });

  test('deep-equals the reviewed compiled Model v18 fixture', () => {
    const source = fixture('model.reviewed.woml');
    expect(
      compileWoml(parseWoml(source, { file: 'model.reviewed.woml' }))
    ).toEqual(JSON.parse(fixture('model.v18.reviewed.json')));
  });

  test('freezes exclusive FIFO ownership, fencing, limits, and failures', () => {
    const contract = JSON.parse(fixture('contracts.v1.json'));
    const events = JSON.parse(fixture('events.v17.json'));
    const errors = JSON.parse(fixture('errors.v1.json'));
    const transitions = JSON.parse(fixture('transitions.v1.json'));
    const threatModel = JSON.parse(fixture('threat-model.v1.json'));
    expect(contract).toMatchObject({
      profile: 'woml.locks-and-leases/v1',
      modelVersion: 18,
      eventVersion: 17,
      storeVersion: 16,
      mode: 'exclusive',
      queue: 'durable-fifo-oldest-eligible-first',
    });
    expect(contract.fencing.targetSideEnforcementRequiredForStrongGuarantee).toBe(
      true
    );
    expect(Object.keys(events.events)).toEqual([
      'lock_wait_registered',
      'lock_acquired',
      'lock_released',
      'lock_acquire_timed_out',
      'lock_acquisition_cancelled',
      'lock_lease_lost',
    ]);
    expect(errors.errors).toContainEqual({
      code: 'WOML_LOCK_LEASE_LOST',
      retryable: false,
      ambiguous: true,
    });
    expect(transitions.races.releaseVsExpiry).toBe('one-transaction-wins');
    expect(threatModel.nonGuarantees).toContain(
      'no-automatic-fencing-for-raw-fetch-or-third-party-code'
    );
  });

  test('parses and lowers a protected multi-step scope with scoped visibility', () => {
    const compiled = compileWoml(
      parseWoml(fixture('model.reviewed.woml'))
    );
    if (compiled.schemaVersion !== 18) throw new Error('Expected Model v18.');
    expect(compiled.graph.locks).toEqual([
      expect.objectContaining({
        lockId: 'deployment',
        bodyEntryNodeId: 'deploy',
        bodyTerminalNodeId: 'verify',
        acquireTimeoutMs: 600_000,
        leaseMs: 30_000,
        outerStepIds: ['prepare'],
      }),
    ]);
    expect(compiled.graph.contextVisibility).toContainEqual({
      nodeId: 'summary',
      stepIds: ['prepare', 'deployment'],
    });
    expect(compiled.graph.contextVisibility).not.toContainEqual({
      nodeId: 'summary',
      stepIds: expect.arrayContaining(['deploy']),
    });
    expect(inspectCompiledWorkflowGraph(compiled.graph)).toEqual([]);
  });

  test('supports locks in choices, fork branches, and for-each templates', () => {
    const compiled = compileWoml(
      parseWoml(fixture('composition.reviewed.woml'))
    );
    if (compiled.schemaVersion !== 18) throw new Error('Expected Model v18.');
    expect(compiled.graph.locks.map(lock => lock.lockId)).toEqual([
      'choiceLock',
      'branchLock',
      'iterationLock',
    ]);
    expect(compiled.graph.forEach[0].body.nodes).toContainEqual(
      expect.objectContaining({ handler: 'engine.lock-acquire' })
    );
  });

  test('accepts imported services inside a lock and emits Definition Model v18', () => {
    const digest = `sha256:${'a'.repeat(64)}`;
    const source = `<woml><imports><module name="maps" from="./maps.ts" /></imports><workflow id="lock-module"><triggers><manual id="start" /></triggers><steps><lock id="geocodeLock" key="maps/global" acquire-timeout="1m"><step id="geocode"><script>return services.maps.lookup("Berlin");</script></step></lock></steps></workflow></woml>`;
    const compiled = compileWomlWithModules(parseWoml(source), {
      profileVersion: 1,
      modules: [
        {
          name: 'maps',
          bundleDigest: digest,
          sourceMapDigest: digest,
          exports: ['lookup'],
        },
      ],
    });
    expect(compiled.schemaVersion).toBe(18);
  });

  test('composes a custom reusable step inside the protected body', async () => {
    const path = resolve(fixtureRoot, 'reusable.reviewed.woml');
    const document = parseWoml(readFileSync(path, 'utf8'), { file: path });
    const graph = resolveWomlReusableDefinitionGraph(document, {
      sourcePath: path,
      projectRoot: fixtureRoot,
    });
    const definitionPackage = await buildWomlReusableDefinitionPackage(
      document,
      graph,
      { sourcePath: path, projectRoot: fixtureRoot }
    );
    expect(definitionPackage.schemaVersion).toBe(13);
    expect(definitionPackage.workflow.model.schemaVersion).toBe(18);
    expect(definitionPackage.workflow.model.graph.nodes).toContainEqual(
      expect.objectContaining({
        id: 'incrementValue',
        handler: 'runtime.script',
        metadata: expect.objectContaining({
          reusableDefinition: expect.objectContaining({ alias: 'increment' }),
        }),
      })
    );
  });

  test('rejects malformed, secret-bearing, nested, duplicate, and invisible lock inputs', () => {
    const validBody = '<step id="work"><script>return true;</script></step>';
    const cases = [
      [
        workflow(`<lock id="resource" acquire-timeout="1m">${validBody}</lock>`),
        'WOML_MISSING_ATTRIBUTE',
      ],
      [
        workflow(`<lock id="resource" key="resource">${validBody}</lock>`),
        'WOML_MISSING_ATTRIBUTE',
      ],
      [
        workflow('<lock id="resource" key="resource" acquire-timeout="1m" />'),
        'WOML_LOCK_EMPTY',
      ],
      [
        workflow(`<lock id="resource" key="/resource" acquire-timeout="1m">${validBody}</lock>`),
        'WOML_LOCK_KEY_INVALID',
      ],
      [
        workflow(`<lock id="resource" key="{{secrets.LOCK_KEY}}" acquire-timeout="1m">${validBody}</lock>`),
        'WOML_LOCK_KEY_SENSITIVE',
      ],
      [
        workflow(`<lock id="resource" key="resource" acquire-timeout="500ms">${validBody}</lock>`),
        'WOML_LOCK_ACQUIRE_TIMEOUT_INVALID',
      ],
      [
        workflow(`<lock id="resource" key="resource" acquire-timeout="1m" lease="4s">${validBody}</lock>`),
        'WOML_LOCK_LEASE_INVALID',
      ],
      [
        workflow(`<lock id="outer" key="outer" acquire-timeout="1m"><lock id="inner" key="inner" acquire-timeout="1m">${validBody}</lock></lock>`),
        'WOML_LOCK_NESTED_UNSUPPORTED',
      ],
      [
        workflow(`<lock id="resource" key="{{context.steps.later.key}}" acquire-timeout="1m">${validBody}</lock><step id="later"><script>return { key: "later" };</script></step>`),
        'WOML_REFERENCE_NOT_DOMINATING',
      ],
      [
        workflow(`<step id="resource"><script>return true;</script></step><lock id="resource" key="resource" acquire-timeout="1m">${validBody}</lock>`),
        'WOML_DUPLICATE_ID',
      ],
    ] as const;
    for (const [source, code] of cases) {
      expect(diagnostic(source).diagnostic.code).toBe(code);
    }
  });

  test('keeps lock diagnostics source-located and exposes editor contracts', () => {
    const source = workflow(
      '<lock id="resource" key="resource" acquire-timeout="1m" lease="1s"><step id="work"><script>return true;</script></step></lock>'
    );
    const error = diagnostic(source);
    expect(error.diagnostic.code).toBe('WOML_LOCK_LEASE_INVALID');
    expect(error.diagnostic.file).toBe('lock-test.woml');
    expect(error.diagnostic.location.start.column).toBeGreaterThan(1);

    const declarations = generateWomlEditorDeclarations([]);
    expect(declarations).toContain('interface WomlLocksService');
    expect(declarations).toContain('readonly locks: WomlLocksService');
    expect(declarations).toContain('readonly withLease');
    const lockDeclarations = generateWomlLockEditorDeclarations();
    expect(lockDeclarations).toContain('readonly fencingToken: string');
    expect(lockDeclarations).toContain('readonly lock:');
  });

  test('rejects a Model v18 graph when its lock descriptor diverges', () => {
    const compiled = compileWoml(
      parseWoml(fixture('model.reviewed.woml'))
    );
    if (compiled.schemaVersion !== 18) throw new Error('Expected Model v18.');
    expect(
      inspectCompiledWorkflowGraph({
        ...compiled.graph,
        locks: [{ ...compiled.graph.locks[0], leaseMs: 1 }],
      })
    ).toContainEqual(expect.objectContaining({ code: 'INVALID_LOCK' }));
  });
});
