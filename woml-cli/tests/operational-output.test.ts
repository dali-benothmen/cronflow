import { describe, expect, test } from 'bun:test';

import {
  decodeOperationalPageV1,
  OperationalOutputError,
  renderOperationalRecord,
  RuntimeOperationalOutput,
  type OperationalPageV1,
  type OperationalRecordV1,
} from '../src/operational-output';
import {
  queryRuntimeOperationalOutput,
  startRuntimeControl,
} from '../src/runtime-control';

const records: readonly OperationalRecordV1[] = [
  {
    profile: 'woml.operational-record/v1',
    deploymentId: 'deployment_test',
    sequence: 1,
    occurredAt: '2026-10-23T10:00:00.000Z',
    workflowId: 'deploy-api',
    runId: 'run_alpha',
    nodeId: 'apply',
    attempt: 1,
    invocationId: 'inv_alpha',
    kind: 'output',
    source: 'stdout',
    text: 'ready\n\u001b]8;;https://host.invalid\u0007hostile',
    byteCount: 42,
  },
  {
    profile: 'woml.operational-record/v1',
    deploymentId: 'deployment_test',
    sequence: 2,
    occurredAt: '2026-10-23T10:00:01.000Z',
    workflowId: 'other-workflow',
    runId: 'run_other',
    kind: 'output',
    source: 'stderr',
    text: 'private to another workflow',
    byteCount: 27,
  },
  {
    profile: 'woml.operational-record/v1',
    deploymentId: 'deployment_test',
    sequence: 3,
    occurredAt: '2026-10-23T10:00:02.000Z',
    workflowId: 'deploy-api',
    runId: 'run_alpha',
    nodeId: 'apply',
    attempt: 1,
    invocationId: 'inv_alpha',
    kind: 'progress',
    progressId: 'rollout',
    current: 2,
    total: 4,
    unit: 'pods',
    message: 'Waiting for readiness',
    status: 'active',
  },
];

function page(after: number): OperationalPageV1 {
  const visible = records.filter(item => item.sequence > after);
  return decodeOperationalPageV1(
    JSON.stringify({
      profile: 'woml.operational-page/v1',
      deploymentId: 'deployment_test',
      requestedAfter: after,
      nextAfter: visible.at(-1)?.sequence ?? after,
      earliestAvailable: 1,
      latestSequence: 3,
      hasMore: false,
      records: visible,
    })
  );
}

function surface(): RuntimeOperationalOutput {
  return new RuntimeOperationalOutput({
    readPage: (_runId, after) => page(after),
    readProgress: () => [],
    readObservation: () => ({
      profile: 'woml.operational-output-store/v1',
      schemaVersion: 1,
      deploymentId: 'deployment_test',
      nextSequence: 4,
      earliestAvailable: 1,
      latestSequence: 3,
      prunedThrough: 0,
      recordCount: 3,
      retainedOutputBytes: 69,
      activeRuns: 1,
      terminalRuns: 1,
      truncatedStreams: 0,
    }),
    heartbeatMs: 60_000,
  });
}

describe('operational output replay and live delivery', () => {
  test('decodes ordered pages and sanitizes hostile terminal content', () => {
    const decoded = page(0);
    expect(decoded.records.map(item => item.sequence)).toEqual([1, 2, 3]);
    const rendered = renderOperationalRecord(decoded.records[0]!, {
      color: 'never',
      includeRunLabel: false,
      unicode: true,
    });
    expect(rendered).toContain('ready');
    expect(rendered).not.toContain('\u001b');
    expect(rendered).not.toContain('\u0007');
    expect(() =>
      decodeOperationalPageV1(
        JSON.stringify({
          ...JSON.parse(JSON.stringify(decoded)),
          records: [...decoded.records].reverse(),
        })
      )
    ).toThrow(OperationalOutputError);
  });

  test('renders safe color, narrow-terminal, and ASCII fallbacks', () => {
    const colored = renderOperationalRecord(records[0]!, {
      color: 'always',
      includeRunLabel: true,
      unicode: true,
      isTTY: true,
      width: 32,
    });
    expect(colored).toContain('\u001b[32mstdout\u001b[0m');
    expect(colored).toEndWith('\u001b[0m…');
    expect(colored).not.toContain('\u001b]');

    const ascii = renderOperationalRecord(records[2]!, {
      color: 'never',
      includeRunLabel: false,
      unicode: false,
      isTTY: false,
      width: 24,
    });
    expect(ascii).toStartWith('[apply] *');
    expect(ascii).toEndWith('.');
    expect(ascii).not.toContain('\u001b[');
  });

  test('scopes workflow replay without exposing another workflow body', async () => {
    const output = surface();
    const response = await queryRuntimeOperationalOutput(
      new URL(
        'http://127.0.0.1/v1/output/workflows/deploy-api?after=0&source=stdout'
      ),
      output
    );
    expect(response).not.toBeInstanceOf(Response);
    const replay = response as OperationalPageV1;
    expect(replay.records.map(item => item.sequence)).toEqual([1, 3]);
    expect(JSON.stringify(replay)).not.toContain('private to another workflow');
    output.closeStreams();
  });

  test('replays after a cursor and limits live followers to eight', async () => {
    const output = surface();
    const responses = Array.from({ length: 8 }, () =>
      output.stream({
        subjectKind: 'run',
        subject: 'run_alpha',
        after: 1,
        sources: new Set(['stdout', 'stderr', 'script', 'runtime']),
      })
    );
    expect(output.followerCount).toBe(8);
    expect(() =>
      output.stream({
        subjectKind: 'run',
        subject: 'run_alpha',
        after: 1,
        sources: new Set(['stdout']),
      })
    ).toThrow('eight operational-output followers');
    const reader = responses[0]!.body!.getReader();
    const first = new TextDecoder().decode((await reader.read()).value);
    expect(first).toContain('"sequence":3');
    expect(first).not.toContain('"sequence":1');
    await reader.cancel();
    output.closeStreams();
    expect(output.followerCount).toBe(0);
  });

  test('validates cursor, source, and route inputs before reading', async () => {
    const output = surface();
    await expect(
      queryRuntimeOperationalOutput(
        new URL('http://127.0.0.1/v1/output/runs/run_alpha?after=-1'),
        output
      )
    ).rejects.toMatchObject({ code: 'WOML_OUTPUT_CURSOR_INVALID' });
    await expect(
      queryRuntimeOperationalOutput(
        new URL('http://127.0.0.1/v1/output/runs/run_alpha?source=secret'),
        output
      )
    ).rejects.toMatchObject({ code: 'WOML_OUTPUT_SOURCE_INVALID' });
    output.closeStreams();
  });

  test('requires the runtime capability for replay and observation endpoints', async () => {
    const output = surface();
    const control = startRuntimeControl({
      runtimeInstanceId: 'runtime_output_test',
      deploymentId: 'deployment_test',
      output,
      port: 0,
    });
    try {
      const url = `${control.descriptor.adminUrl}/v1/output/runs/run_alpha?after=0`;
      expect((await fetch(url)).status).toBe(401);
      const authorized = await fetch(url, {
        headers: {
          authorization: `Bearer ${control.descriptor.capability}`,
        },
      });
      expect(authorized.status).toBe(200);
      const replay = (await authorized.json()) as OperationalPageV1;
      expect(replay.profile).toBe('woml.operational-page/v1');
      expect(replay.records.every(item => item.runId === 'run_alpha')).toBe(true);
      const forged = await fetch(url, {
        headers: { authorization: `Bearer ${'x'.repeat(43)}` },
      });
      expect(forged.status).toBe(401);
    } finally {
      await control.close();
    }
  });
});
