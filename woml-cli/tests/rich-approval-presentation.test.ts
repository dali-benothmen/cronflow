import { describe, expect, test } from 'bun:test';

import {
  assertRichApprovalNotificationV3,
  createRichApprovalPresentation,
  presentationContainsForbiddenValue,
  renderRichApprovalNotification,
  type RichApprovalNotificationV3,
} from '../src/rich-approval-presentation';

const digest = `sha256:${'a'.repeat(64)}`;

function invocation(
  provider: RichApprovalNotificationV3['provider'] = 'slack'
): RichApprovalNotificationV3 {
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
      artifacts: [{
        contract: 'woml.artifact-reference', contractVersion: 1,
        id: 'art_plan', name: 'production.tfplan', size: 4096,
        sha256: `sha256:${'b'.repeat(64)}`, contentType: 'application/octet-stream',
      }],
    },
    { required: 2, received: 1, requiredRoles: ['production-owner', 'security-owner'] }
  );
  return {
    protocol: 'woml.notification-provider-host',
    protocolVersion: 3,
    messageType: 'deliver',
    mode: 'rich-approval',
    invocationId: 'inv_review_1',
    runId: 'run_review_1',
    approvalId: 'approveDeployment',
    requestId: 'aprreq_review_1',
    deliveryId: 'approveDeployment:notify:0:channel:0',
    provider,
    destination: provider === 'telegram' ? '123456789' : 'C0123456789',
    reviewDigest: digest,
    presentation,
    decisionCapability: {
      url: 'http://127.0.0.1:7331/api/v2/approvals/run_review_1/aprreq_review_1',
      token: 'a'.repeat(32),
      expiresAt: '2026-10-21T12:30:00Z',
    },
  };
}

describe('Rich Approval Provider Host v3 presentation', () => {
  test('renders one deterministic bounded evidence summary for every provider', () => {
    for (const provider of ['slack', 'telegram', 'discord', 'custom'] as const) {
      const message = invocation(provider);
      expect(() => assertRichApprovalNotificationV3(message)).not.toThrow();
      const rendered = renderRichApprovalNotification(message);
      expect(rendered.text).toContain('Deploy payments-api to production');
      expect(rendered.text).toContain('Quorum: 1/2');
      expect(rendered.text).toContain('Key changes');
      expect(rendered.text).toContain('+1 more change(s)');
      expect(rendered.text).toContain('Terraform validation');
      expect(rendered.text).toContain('production.tfplan');
      expect(rendered.text).toContain('aaaaaaaaaaaa…');
      expect([...rendered.text].length).toBeLessThanOrEqual(provider === 'discord' ? 2_000 : 4_096);
      if (provider === 'slack') {
        expect(rendered.blocks).toBeDefined();
      } else {
        expect(rendered.blocks).toBeUndefined();
      }
    }
  });

  test('uses plain text blocks, removes active control characters, and never copies capabilities into presentation', () => {
    const message = invocation();
    const presentation = createRichApprovalPresentation(
      {
        title: 'Deploy <script>alert(1)</script>\u202e',
        summary: 'safe summary',
        risk: 'critical',
      },
      message.presentation.authority
    );
    expect(presentation.title).toBe('Deploy <script>alert(1)</script>�');
    expect(presentationContainsForbiddenValue(presentation, [
      message.decisionCapability.token,
      'super-secret-value',
    ])).toBe(false);
    const rendered = renderRichApprovalNotification({ ...message, presentation });
    expect(JSON.stringify(rendered.blocks)).toContain('plain_text');
    expect(JSON.stringify(rendered.blocks)).not.toContain(message.decisionCapability.token);
  });

  test('rejects stale, unknown, and over-quorum envelopes', () => {
    const valid = invocation();
    expect(() => assertRichApprovalNotificationV3({
      ...valid,
      reviewDigest: 'sha256:stale',
    })).toThrow('invalid rich approval presentation');
    expect(() => assertRichApprovalNotificationV3({
      ...valid,
      presentation: {
        ...valid.presentation,
        authority: { ...valid.presentation.authority, received: 3 },
      },
    })).toThrow('invalid rich approval presentation');
    expect(() => assertRichApprovalNotificationV3({
      ...valid,
      secretValues: ['must-not-exist'],
    })).toThrow('invalid rich approval presentation');
  });
});
