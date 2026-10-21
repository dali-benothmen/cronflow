import { describe, expect, test } from 'bun:test';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import {
  assertRichApprovalNotificationV3,
  createRichApprovalPresentation,
  presentationContainsForbiddenValue,
  renderRichApprovalNotification,
  type RichApprovalNotificationV3,
} from '../src/rich-approval-presentation';

function invocation(): RichApprovalNotificationV3 {
  return {
    protocol: 'woml.notification-provider-host',
    protocolVersion: 3,
    messageType: 'deliver',
    mode: 'rich-approval',
    invocationId: 'inv_review_1',
    runId: 'run_review_1',
    approvalId: 'approveDeployment',
    requestId: 'aprreq_review_1',
    deliveryId: 'delivery_review_1',
    provider: 'slack',
    destination: 'C0123456789',
    reviewDigest:
      'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    presentation: {
      title: 'Deploy payments-api',
      summary: 'Apply the exact reviewed production plan.',
      risk: 'high',
      authority: {
        required: 2,
        received: 0,
        requiredRoles: ['production-owner', 'security-owner'],
      },
    },
    decisionCapability: {
      url: 'http://127.0.0.1:3000/api/v2/approvals/request-1',
      token: 'rpa_0123456789abcdef0123456789abcdef',
      expiresAt: '2099-01-01T00:00:00.000Z',
    },
  };
}

describe('Rich Approval Provider Host v3 hardening', () => {
  test('renders one deterministic bounded evidence summary for every provider', () => {
    const presentation = createRichApprovalPresentation(
      {
        title: 'Deploy payments-api to production',
        summary: 'Apply the reviewed production infrastructure plan.',
        risk: 'high',
        changes: [
          { action: 'create', target: 'aws_db_instance.primary', summary: 'Create the primary database.' },
          { action: 'update', target: 'aws_ecs_service.api', summary: 'Roll out the new task definition.' },
          { action: 'destroy', target: 'aws_instance.legacy', summary: 'Remove the retired host.' },
          { action: 'no-op', target: 'aws_s3_bucket.audit', summary: 'No change.' },
        ],
        checks: [{ name: 'Terraform validation', status: 'passed' }],
        artifacts: [{ name: 'production.tfplan', size: 4096 }],
      },
      { required: 2, received: 1, requiredRoles: ['production-owner', 'security-owner'] }
    );
    for (const provider of ['slack', 'telegram', 'discord', 'custom'] as const) {
      const message = { ...invocation(), provider, presentation };
      expect(() => assertRichApprovalNotificationV3(message)).not.toThrow();
      const rendered = renderRichApprovalNotification(message);
      expect(rendered.text).toContain('Deploy payments-api to production');
      expect(rendered.text).toContain('Quorum: 1/2');
      expect(rendered.text).toContain('Key changes');
      expect(rendered.text).toContain('+1 more change(s)');
      expect(rendered.text).toContain('Terraform validation');
      expect(rendered.text).toContain('production.tfplan');
      expect([...rendered.text].length).toBeLessThanOrEqual(
        provider === 'discord' ? 2_000 : 4_096
      );
      expect(rendered.blocks === undefined).toBe(provider !== 'slack');
    }
  });

  test('rejects malformed and confused invocation envelopes without exposing capabilities', () => {
    const valid = invocation();
    const mutations: unknown[] = [
      { ...valid, unexpected: true },
      { ...valid, protocolVersion: 2 },
      { ...valid, provider: 'email' },
      { ...valid, runId: '' },
      { ...valid, destination: 'x'.repeat(321) },
      { ...valid, reviewDigest: `sha256:${'A'.repeat(64)}` },
      { ...valid, presentation: { ...valid.presentation, risk: 'unknown' } },
      {
        ...valid,
        presentation: {
          ...valid.presentation,
          authority: { required: 2, received: 3, requiredRoles: [] },
        },
      },
      {
        ...valid,
        decisionCapability: { ...valid.decisionCapability, url: 'not a URL' },
      },
      {
        ...valid,
        decisionCapability: { ...valid.decisionCapability, token: 'short' },
      },
      {
        ...valid,
        decisionCapability: {
          ...valid.decisionCapability,
          expiresAt: 'not-a-time',
        },
      },
    ];

    for (const mutation of mutations) {
      let message = '';
      try {
        assertRichApprovalNotificationV3(mutation);
      } catch (error) {
        message = error instanceof Error ? error.message : String(error);
      }
      expect(message).toContain('invalid rich approval presentation');
      expect(message).not.toContain(valid.decisionCapability.token);
    }
  });

  test('renders markup as text and neutralizes control and bidirectional characters', () => {
    const presentation = createRichApprovalPresentation(
      {
        title: '<script>alert(1)</script>\u202e',
        summary: 'Deploy\u0007 only the reviewed plan.',
        risk: 'critical',
        changes: [],
        checks: [],
        artifacts: [],
      },
      { required: 1, received: 0, requiredRoles: ['operator\u202e'] }
    );
    const rendered = renderRichApprovalNotification({
      ...invocation(),
      presentation,
    });

    expect(rendered.text).toContain('<script>alert(1)</script>�');
    expect(rendered.text).not.toContain('\u0007');
    expect(rendered.text).not.toContain('\u202e');
    expect(presentationContainsForbiddenValue(presentation, [
      invocation().decisionCapability.token,
      'super-secret-value',
    ])).toBe(false);
    expect(JSON.stringify(rendered)).not.toContain(
      invocation().decisionCapability.token
    );
  });

  test('stays within the reviewed rendering budget', async () => {
    const budgets = JSON.parse(
      await readFile(
        resolve(import.meta.dir, '../../examples/richApprovals/performance-budgets.v1.json'),
        'utf8'
      )
    ) as { budgets: { renderingP95Ms: number } };
    const samples: number[] = [];
    for (let index = 0; index < 100; index += 1) {
      const started = performance.now();
      renderRichApprovalNotification(invocation());
      samples.push(performance.now() - started);
    }
    samples.sort((left, right) => left - right);
    const p95 = samples[Math.ceil(samples.length * 0.95) - 1]!;
    expect(p95).toBeLessThan(budgets.budgets.renderingP95Ms);
  });
});
