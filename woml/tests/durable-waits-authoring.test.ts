import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import Ajv2020 from 'ajv/dist/2020';

import {
  compileWoml,
  inspectCompiledWorkflowGraph,
  parseWoml,
  WomlCompileError,
  WomlValidationError,
} from '../src';

function fixture(name: string): string {
  return readFileSync(
    new URL(`./fixtures/durable-waits/${name}`, import.meta.url),
    'utf8'
  );
}

function workflow(body: string, before = ''): string {
  return `<woml><workflow id="durable-wait-test"><triggers><manual id="start" /></triggers><steps>${before}${body}<step id="done"><script>return true;</script></step></steps></workflow></woml>`;
}

function diagnostic(source: string): WomlValidationError | WomlCompileError {
  try {
    compileWoml(parseWoml(source, { file: 'durable-wait-test.woml' }));
  } catch (error) {
    if (
      error instanceof WomlValidationError ||
      error instanceof WomlCompileError
    ) {
      return error;
    }
    throw error;
  }
  throw new Error('Expected durable wait authoring to fail.');
}

describe('WOML durable waits DW0 and DW1', () => {
  test('freezes payload-free wait inspection and run inspection v8 schemas', () => {
    const schemaRoot = resolve(import.meta.dir, '../../docs/schemas');
    const ajv = new Ajv2020({ allErrors: true, strict: false });
    ajv.addFormat('date-time', {
      validate: (value: string) => Number.isFinite(Date.parse(value)),
    });
    for (const name of [
      'artifact-reference.v1.schema.json',
      'artifact-inspection.v1.schema.json',
      'artifact-run-inspection.v1.schema.json',
      'wait-inspection.v1.schema.json',
      'run-inspection.v8.schema.json',
    ]) {
      ajv.addSchema(
        JSON.parse(readFileSync(resolve(schemaRoot, name), 'utf8'))
      );
    }
    const validate = ajv.getSchema('https://woml.dev/schemas/run-inspection/v8');
    if (validate === undefined) throw new Error('Missing run inspection v8 schema.');
    const inspected = {
      profile: 'woml.run-inspection/v8',
      runId: 'run_waiting',
      workflowId: 'wait-composition',
      status: 'waiting',
      businessOutcome: 'undecided',
      lifecycleStatus: 'idle',
      hooks: [],
      warnings: [],
      cancellation: { requested: false },
      artifacts: {
        profile: 'woml.artifact-run-inspection/v1',
        runId: 'run_waiting',
        artifacts: [],
        truncated: false,
      },
      durableWaits: {
        profile: 'woml.wait-inspection/v1',
        runId: 'run_waiting',
        pendingWaits: 1,
        oldestPendingWaitAgeMs: 250,
        waits: [
          {
            runId: 'run_waiting',
            waitId: 'scanResult',
            name: 'Wait for scan result',
            kind: 'signal',
            status: 'pending',
            registeredAt: '2026-10-13T10:00:00Z',
            signals: ['scan.completed'],
            correlationDigest:
              'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            deadline: '2026-10-13T11:00:00Z',
          },
        ],
      },
    };
    expect(validate(inspected), JSON.stringify(validate.errors)).toBe(true);
    expect(validate({ ...inspected, payload: { secret: true } })).toBe(false);
  });

  test('deep-equals the reviewed compiled Model v17 fixture', () => {
    const source = fixture('model.reviewed.woml');
    expect(
      compileWoml(parseWoml(source, { file: 'model.reviewed.woml' }))
    ).toEqual(JSON.parse(fixture('model.v17.reviewed.json')));
  });

  test('freezes the reviewed product limits and version successors', () => {
    const contract = JSON.parse(fixture('contracts.v1.json'));
    const events = JSON.parse(fixture('events.v16.json'));
    const signalApi = JSON.parse(fixture('signal-api.v1.json'));
    const inspection = JSON.parse(fixture('inspection.v1.json'));
    expect(contract).toMatchObject({
      profile: 'woml.durable-waits-contract/v1',
      modelVersion: 17,
      eventVersion: 16,
      storeVersion: 15,
      admissionConcurrency: 'waiting-runs-do-not-consume-execution-capacity',
    });
    expect(contract.resolution.cardinality).toBe('exactly-one');
    expect(contract.security.runtimeAdminCredentialReuse).toBe(false);
    expect(Object.keys(events.events)).toEqual([
      'wait_registered',
      'external_signal_received',
      'wait_satisfied',
      'wait_timed_out',
      'wait_cancelled',
    ]);
    expect(signalApi.http.statusMap).toMatchObject({
      accepted: 202,
      duplicate: 200,
      identity_conflict: 409,
    });
    expect(inspection.forbidden).toContain('payload');
  });

  test('parses and lowers every reviewed authoring form to Model v17', () => {
    const source = fixture('authoring.reviewed.woml');
    const compiled = compileWoml(
      parseWoml(source, { file: 'authoring.reviewed.woml' })
    );
    if (compiled.schemaVersion !== 17) throw new Error('Expected Model v17.');
    expect(compiled.graph.waits).toEqual([
      expect.objectContaining({
        waitId: 'observeCanary',
        kind: 'duration',
        durationMs: 600_000,
      }),
      expect.objectContaining({
        waitId: 'maintenanceWindow',
        kind: 'until',
        at: {
          kind: 'contextReference',
          path: ['steps', 'startScan', 'windowStart'],
        },
      }),
      expect.objectContaining({
        waitId: 'securityScan',
        kind: 'signal',
        timeoutMs: 7_200_000,
        onTimeout: 'fail',
      }),
      expect.objectContaining({
        waitId: 'buildResult',
        kind: 'signal',
        timeoutMs: 3_600_000,
        onTimeout: 'continue',
      }),
    ]);
    expect(
      compiled.graph.nodes.filter(
        node => node.handler === 'engine.durable-wait'
      )
    ).toHaveLength(4);
    expect(compiled.graph.contextVisibility).toContainEqual({
      nodeId: 'summary',
      stepIds: [
        'startScan',
        'observeCanary',
        'maintenanceWindow',
        'securityScan',
        'buildResult',
      ],
    });
  });

  test('rejects malformed durations, times, signal forms, schemas, and timeout policies', () => {
    const cases = [
      [
        workflow('<wait id="pause" duration="0s" />'),
        'WOML_WAIT_DURATION_INVALID',
      ],
      [
        workflow('<wait-until id="pause" at="tomorrow" />'),
        'WOML_WAIT_TIME_INVALID',
      ],
      [
        workflow('<wait-for id="pause" correlation="job-1" />'),
        'WOML_WAIT_SIGNAL_REQUIRED',
      ],
      [
        workflow(
          '<wait-for id="pause" signal="job.done" correlation="job-1"><accept signal="job.failed" /></wait-for>'
        ),
        'WOML_WAIT_SIGNAL_FORM_CONFLICT',
      ],
      [
        workflow(
          '<wait-for id="pause" correlation="job-1"><accept signal="job.done" /><accept signal="job.done" /></wait-for>'
        ),
        'WOML_WAIT_SIGNAL_DUPLICATE',
      ],
      [
        workflow(
          '<wait-for id="pause" signal="JobDone" correlation="job-1" />'
        ),
        'WOML_SIGNAL_NAME_INVALID',
      ],
      [
        workflow(
          '<wait-for id="pause" signal="job.done" correlation=" job-1" />'
        ),
        'WOML_SIGNAL_CORRELATION_INVALID',
      ],
      [
        workflow(
          '<wait-for id="pause" signal="job.done" correlation="job-1"><schema>{"type":7}</schema></wait-for>'
        ),
        'WOML_SIGNAL_SCHEMA_INVALID',
      ],
      [
        workflow(
          '<wait-for id="pause" signal="job.done" correlation="job-1" on-timeout="continue" />'
        ),
        'WOML_WAIT_TIMEOUT_POLICY_INVALID',
      ],
      [
        workflow(
          '<wait-for id="pause" signal="job.done" correlation="{{context.item.id}}" />'
        ),
        'WOML_SIGNAL_CORRELATION_INVALID',
      ],
    ] as const;
    for (const [source, code] of cases) {
      expect(diagnostic(source).diagnostic.code).toBe(code);
    }
  });

  test('requires dynamic wait inputs to be visible at their graph position', () => {
    const source = workflow(
      '<wait-for id="pause" signal="job.done" correlation="{{context.steps.startJob.id}}" />',
      ''
    ).replace(
      '<step id="done">',
      '<step id="startJob"><script>return { id: "job-1" };</script></step><step id="done">'
    );
    expect(diagnostic(source).diagnostic.code).toBe(
      'WOML_REFERENCE_NOT_DOMINATING'
    );
  });

  test('keeps diagnostics source-located at the invalid attribute', () => {
    const source = workflow('<wait id="pause" duration="later" />');
    const error = diagnostic(source);
    expect(error.diagnostic.file).toBe('durable-wait-test.woml');
    expect(error.diagnostic.location.start.line).toBe(1);
    expect(error.diagnostic.location.start.column).toBeGreaterThan(1);
  });

  test('composes waits inside choices, forks, and for-each body templates', () => {
    const source = `<woml><workflow id="wait-composition"><triggers><manual id="start" /></triggers><steps>
      <step id="load"><script>return { items: [{ id: "one" }] };</script></step>
      <choose>
        <when test="{{context.payload.delayed}}"><wait id="choiceDelay" duration="1s" /></when>
        <otherwise><step id="immediate"><script>return true;</script></step></otherwise>
      </choose>
      <fork id="routes" join="all"><branch id="audit"><wait id="auditDelay" duration="1s" /></branch></fork>
      <for-each id="jobs" items="{{context.steps.load.items}}">
        <wait-for id="jobResult" signal="job.completed" correlation="{{context.item.id}}" timeout="1h" />
      </for-each>
      <step id="done"><script>return context.steps.jobs;</script></step>
    </steps></workflow></woml>`;
    const compiled = compileWoml(parseWoml(source));
    if (compiled.schemaVersion !== 17) throw new Error('Expected Model v17.');
    expect(compiled.graph.waits.map(wait => wait.waitId)).toEqual([
      'choiceDelay',
      'auditDelay',
      'jobResult',
    ]);
    expect(compiled.graph.forEach[0].body.nodes).toContainEqual(
      expect.objectContaining({
        id: 'jobResult',
        handler: 'engine.durable-wait',
      })
    );
  });

  test('rejects a Model v17 graph when wait descriptors and nodes diverge', () => {
    const compiled = compileWoml(
      parseWoml(workflow('<wait id="pause" duration="1s" />'))
    );
    if (compiled.schemaVersion !== 17) throw new Error('Expected Model v17.');
    expect(
      inspectCompiledWorkflowGraph({ ...compiled.graph, waits: [] })
    ).toContainEqual(expect.objectContaining({ code: 'INVALID_DURABLE_WAIT' }));
  });
});
