import { describe, expect, test } from 'bun:test';

import {
  serveApprovalAndWait,
  type ApprovalWaitReason,
} from '../src/approval-server';
import {
  ApprovalDecisionError,
  type ApprovalDecision,
  type ApprovalDecisionResult,
  type RichApprovalDecisionResult,
  type RustApprovalRuntimeOutcome,
} from '../src/rust-executor';

type WaitingOutcome = Extract<
  RustApprovalRuntimeOutcome,
  { status: 'waiting' }
>;

const token =
  'apr_0123456789abcdef0123456789abcdef.0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';

function waitingOutcome(): WaitingOutcome {
  return {
    contract: 'woml.runtime-outcome',
    version: 1,
    status: 'waiting',
    workflowId: 'publish-article',
    runId: 'run_approval_http',
    approval: {
      approvalId: 'editorApproval',
      requestId: 'aprreq_editor_http',
      name: 'Editorial approval',
      description: 'Approve publication',
      onTimeout: 'reject',
      token,
      credentialExpiresAt: '2099-01-01T00:00:00.000Z',
    },
  };
}

function richWaitingOutcome(): WaitingOutcome {
  return {
    contract: 'woml.runtime-outcome',
    version: 1,
    status: 'waiting',
    workflowId: 'production-deployment',
    runId: 'run_rich_approval_http',
    approval: {
      approvalId: 'approveDeployment',
      requestId: 'aprreq_review_http',
      name: 'Approve production deployment',
      description: 'Authorize the exact reviewed plan.',
      expiresAt: '2099-01-01T00:00:00.000Z',
      onTimeout: 'reject',
      reviewDigest:
        'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      review: {
        title: 'Deploy payments-api to production',
        summary: 'Create 2, modify 4, and destroy 1 resource.',
        risk: 'high',
        facts: [],
        changes: [],
        checks: [],
        artifacts: [],
      },
      authority: {
        required: 2,
        received: 0,
        requiredRoles: ['production-owner', 'security-owner'],
        representedRoles: [],
      },
    },
  };
}

function richDecisionResult(
  status: RichApprovalDecisionResult['status']
): RichApprovalDecisionResult {
  return {
    contract: 'woml.approval-http',
    version: 2,
    status,
    runId: 'run_rich_approval_http',
    approvalId: 'approveDeployment',
    requestId: 'aprreq_review_http',
    reviewDigest:
      'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  };
}

function decisionResult(
  decision: ApprovalDecision,
  status: 'accepted' | 'already_resolved' = 'accepted'
): ApprovalDecisionResult {
  return {
    contract: 'woml.approval-http',
    version: 1,
    status,
    runId: 'run_approval_http',
    approvalId: 'editorApproval',
    requestId: 'aprreq_editor_http',
    decision,
    source: 'human',
    decidedAt: '2026-08-07T10:05:00.000Z',
  };
}

async function availablePort(): Promise<number> {
  const probe = Bun.serve({
    hostname: '127.0.0.1',
    port: 0,
    fetch: () => new Response(),
  });
  const port = probe.port;
  probe.stop(true);
  if (port === undefined) throw new Error('Bun did not assign a test port.');
  return port;
}

async function serverJourney(
  onDecision: (
    token: string,
    decision: ApprovalDecision
  ) => ApprovalDecisionResult
): Promise<{
  readonly url: string;
  readonly wait: Promise<ApprovalWaitReason>;
}> {
  const port = await availablePort();
  let listening!: (url: string) => void;
  const url = new Promise<string>(resolve => {
    listening = resolve;
  });
  const wait = serveApprovalAndWait({
    outcome: waitingOutcome(),
    port,
    onDecision,
    onTimeout: () => ({
      status: 'not_due',
      runId: 'run_approval_http',
      approvalId: 'editorApproval',
      requestId: 'aprreq_editor_http',
      resolution: null,
      settledAt: null,
    }),
    onListening: listening,
  });
  return { url: await url, wait };
}

function post(url: string, body: string, headers?: HeadersInit) {
  const tokenUrl = new URL(url);
  return fetch(
    `${tokenUrl.origin}/api/v1/approvals/${tokenUrl.pathname.slice(
      '/approvals/'.length
    )}/decision`,
    { method: 'POST', headers, body }
  );
}

describe('local approval HTTP v1', () => {
  test('accepts a provider capability through a direct HTTP URL', async () => {
    const port = await availablePort();
    let listening!: (url: string) => void;
    const url = new Promise<string>(resolve => {
      listening = resolve;
    });
    const decisions: string[] = [];
    const wait = serveApprovalAndWait({
      outcome: waitingOutcome(),
      port,
      onDecision: (_token, decision) => decisionResult(decision),
      onNotificationDecision: (capability, decision) => {
        decisions.push(`${capability}:${decision}`);
        return decisionResult(decision);
      },
      onTimeout: () => ({
        status: 'not_due',
        runId: 'run_approval_http',
        approvalId: 'editorApproval',
        requestId: 'aprreq_editor_http',
        resolution: null,
        settledAt: null,
      }),
      onListening: listening,
    });
    const origin = new URL(await url).origin;
    const response = await fetch(
      `${origin}/api/v1/notification-approvals/ncap_provider_42/approved`,
      { method: 'POST' }
    );
    expect(response.status).toBe(200);
    expect(decisions).toEqual(['ncap_provider_42:approved']);
    expect(await wait).toBe('decision');
  });

  test('serves a read-only secured page and rejects malformed requests', async () => {
    const journey = await serverJourney((_token, decision) =>
      decisionResult(decision)
    );

    const page = await fetch(journey.url);
    expect(page.status).toBe(200);
    expect(page.headers.get('cache-control')).toBe('no-store');
    expect(page.headers.get('referrer-policy')).toBe('no-referrer');
    expect(page.headers.get('x-frame-options')).toBe('DENY');
    expect(page.headers.get('content-security-policy')).toContain(
      "default-src 'none'"
    );
    const html = await page.text();
    expect(html).toContain('Editorial approval');
    expect(html).not.toContain('context.steps');
    expect(
      (await fetch(`${new URL(journey.url).origin}/approvals/not-a-token`))
        .status
    ).toBe(404);

    expect((await post(journey.url, '{}')).status).toBe(400);
    expect(
      (
        await post(journey.url, '{', {
          'Content-Type': 'application/json',
        })
      ).status
    ).toBe(400);
    expect(
      (
        await post(journey.url, '{"decision":"approved"}', {
          'Content-Type': 'application/json',
          Origin: 'http://example.com',
        })
      ).status
    ).toBe(400);

    const accepted = await post(journey.url, '{"decision":"approved"}', {
      'Content-Type': 'application/json',
    });
    expect(accepted.status).toBe(200);
    expect(await journey.wait).toBe('decision');
  });

  test('returns accepted and idempotent responses for simultaneous duplicates', async () => {
    let calls = 0;
    const journey = await serverJourney((_token, decision) =>
      decisionResult(decision, calls++ === 0 ? 'accepted' : 'already_resolved')
    );
    const responses = await Promise.all([
      post(journey.url, '{"decision":"approved"}', {
        'Content-Type': 'application/json',
      }),
      post(journey.url, '{"decision":"approved"}', {
        'Content-Type': 'application/json',
      }),
    ]);
    expect(responses.map(response => response.status)).toEqual([200, 200]);
    const bodies = await Promise.all(
      responses.map(response => response.json())
    );
    expect(bodies.map(body => body.status).sort()).toEqual([
      'accepted',
      'already_resolved',
    ]);
    await journey.wait;
  });

  test('maps conflicting and expired Rust decisions without leaking tokens', async () => {
    for (const [code, expectedStatus] of [
      ['WOML_APPROVAL_DECISION_CONFLICT', 409],
      ['WOML_APPROVAL_TOKEN_EXPIRED', 410],
      ['WOML_APPROVAL_EXPIRED', 410],
    ] as const) {
      const journey = await serverJourney(() => {
        throw new ApprovalDecisionError(code, 'sensitive internal detail');
      });
      const response = await post(journey.url, '{"decision":"rejected"}', {
        'Content-Type': 'application/json',
      });
      expect(response.status).toBe(expectedStatus);
      const body = await response.text();
      expect(body).toContain(code);
      expect(body).not.toContain(token);
      expect(await journey.wait).toBe('refresh');
    }
  });

  test('rotates an expiring capability without resolving the approval', async () => {
    const port = await availablePort();
    const outcome = waitingOutcome();
    const wait = serveApprovalAndWait({
      outcome: {
        ...outcome,
        approval: {
          ...outcome.approval,
          credentialExpiresAt: new Date(Date.now() + 30).toISOString(),
        },
      },
      port,
      onDecision: (_token, decision) => decisionResult(decision),
      onTimeout: () => {
        throw new Error('Credential refresh must not settle the approval.');
      },
      onListening: () => {},
    });
    expect(await wait).toBe('refresh');
  });
});

describe('trusted approval HTTP v2', () => {
  test('rejects malformed, oversized, and digest-confused decisions before authority admission', async () => {
    const port = await availablePort();
    const controller = new AbortController();
    let listening!: (url: string) => void;
    const urlReady = new Promise<string>(resolve => {
      listening = resolve;
    });
    let admissions = 0;
    const wait = serveApprovalAndWait({
      outcome: richWaitingOutcome(),
      port,
      signal: controller.signal,
      onDecision: () => decisionResult('approved'),
      verifyRichIdentity: () => ({
        provider: 'slack',
        providerAccountId: 'T0123456789',
        providerActorId: 'U0123456789',
      }),
      onRichDecision: () => {
        admissions += 1;
        return richDecisionResult('recorded');
      },
      onTimeout: () => ({
        status: 'not_due',
        runId: 'run_rich_approval_http',
        approvalId: 'approveDeployment',
        requestId: 'aprreq_review_http',
        resolution: null,
        settledAt: null,
      }),
      onListening: listening,
    });
    const url = await urlReady;
    const digest = richWaitingOutcome().approval.reviewDigest!;
    const bodies = [
      '',
      '{',
      'null',
      '[]',
      '{}',
      JSON.stringify({
        decision: 'maybe', reviewDigest: digest,
        gateway: 'slack-signed-callback', assertion: 'signed-1',
      }),
      JSON.stringify({
        decision: 'approved', reviewDigest: `sha256:${'A'.repeat(64)}`,
        gateway: 'slack-signed-callback', assertion: 'signed-1',
      }),
      JSON.stringify({
        decision: 'approved', reviewDigest: digest,
        gateway: 'slack-signed-callback', assertion: 'signed-1', extra: true,
      }),
      JSON.stringify({
        decision: 'approved', reviewDigest: digest,
        gateway: '', assertion: 'signed-1',
      }),
      JSON.stringify({
        decision: 'approved', reviewDigest: digest,
        gateway: 'slack-signed-callback', assertion: 'x'.repeat(70_000),
      }),
    ];
    for (const body of bodies) {
      const response = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
      });
      expect([400, 413]).toContain(response.status);
      const responseBody = await response.text();
      expect(responseBody).not.toContain('signed-1');
    }
    const wrongMethod = await fetch(url, { method: 'PUT' });
    expect([404, 405]).toContain(wrongMethod.status);
    expect(admissions).toBe(0);
    controller.abort();
    expect(await wait).toBe('refresh');
  });

  test('renders the frozen review and admits only verified digest-bound identities', async () => {
    const port = await availablePort();
    let listening!: (url: string) => void;
    const urlReady = new Promise<string>(resolve => {
      listening = resolve;
    });
    const verified: string[] = [];
    let decisions = 0;
    const wait = serveApprovalAndWait({
      outcome: richWaitingOutcome(),
      port,
      onDecision: () => {
        throw new Error(
          'Rich approval must not use the basic token authority.'
        );
      },
      verifyRichIdentity: (gateway, assertion) => {
        verified.push(`${gateway}:${assertion}`);
        if (gateway !== 'slack-signed-callback' || assertion !== 'signed-1') {
          throw new ApprovalDecisionError(
            'WOML_APPROVAL_REVIEWER_UNKNOWN',
            'untrusted assertion'
          );
        }
        return {
          provider: 'slack',
          providerAccountId: 'T0123456789',
          providerActorId: 'U0123456789',
        };
      },
      onRichDecision: request => {
        expect(request.reviewDigest).toBe(
          richWaitingOutcome().approval.reviewDigest!
        );
        expect(request.identity.providerAccountId).toBe('T0123456789');
        decisions += 1;
        return richDecisionResult(decisions === 1 ? 'recorded' : 'resolved');
      },
      onTimeout: () => ({
        status: 'not_due',
        runId: 'run_rich_approval_http',
        approvalId: 'approveDeployment',
        requestId: 'aprreq_review_http',
        resolution: null,
        settledAt: null,
      }),
      onListening: listening,
    });
    const url = await urlReady;
    expect(url).toContain('/api/v2/approvals/');
    expect(url).not.toContain('apr_');

    const reviewResponse = await fetch(url);
    expect(reviewResponse.status).toBe(200);
    const review = await reviewResponse.json();
    expect(review.version).toBe(2);
    expect(review.review.title).toBe('Deploy payments-api to production');
    expect(review.authority).toEqual(richWaitingOutcome().approval.authority);

    const pageResponse = await fetch(url, {
      headers: { Accept: 'text/html' },
    });
    expect(pageResponse.status).toBe(200);
    expect(pageResponse.headers.get('content-security-policy')).toContain(
      "default-src 'none'"
    );
    const page = await pageResponse.text();
    expect(page).toContain('Deploy payments-api to production');
    expect(page).toContain('Quorum');
    expect(page).toContain('Quorum 0/2');
    expect(page).toContain('production-owner');
    expect(page).toContain('aaaaaaaaaaaa');
    expect(page).not.toContain('signed-1');
    expect(page).not.toContain('providerActorId');

    const stale = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        decision: 'approved',
        reviewDigest:
          'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
        gateway: 'slack-signed-callback',
        assertion: 'signed-1',
      }),
    });
    expect(stale.status).toBe(400);
    expect(verified).toEqual([]);

    const vote = () =>
      fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          decision: 'approved',
          reviewDigest: richWaitingOutcome().approval.reviewDigest,
          gateway: 'slack-signed-callback',
          assertion: 'signed-1',
        }),
      });
    expect((await (await vote()).json()).status).toBe('recorded');
    expect((await (await vote()).json()).status).toBe('resolved');
    expect(await wait).toBe('decision');
    expect(verified).toHaveLength(2);
  });

  test('fails closed when no trusted identity gateway is configured', async () => {
    const port = await availablePort();
    const controller = new AbortController();
    let listening!: (url: string) => void;
    const urlReady = new Promise<string>(resolve => {
      listening = resolve;
    });
    const wait = serveApprovalAndWait({
      outcome: richWaitingOutcome(),
      port,
      signal: controller.signal,
      onDecision: () => decisionResult('approved'),
      onTimeout: () => ({
        status: 'not_due',
        runId: 'run_rich_approval_http',
        approvalId: 'approveDeployment',
        requestId: 'aprreq_review_http',
        resolution: null,
        settledAt: null,
      }),
      onListening: listening,
    });
    const url = await urlReady;
    const response = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    });
    expect(response.status).toBe(503);
    expect(await response.text()).toContain(
      'WOML_APPROVAL_AUTHORITY_UNAVAILABLE'
    );
    controller.abort();
    expect(await wait).toBe('refresh');
  });
});
