import type { JsonObject, JsonValue } from '@woml/compiler';

export const RICH_NOTIFICATION_PROVIDER_PROTOCOL_VERSION = 3 as const;
const SHA256 = /^sha256:[a-f0-9]{64}$/;
const CONTROL_OR_BIDI = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u202a-\u202e\u2066-\u2069]/gu;

export interface RichApprovalAuthorityPresentation {
  readonly required: number;
  readonly received: number;
  readonly requiredRoles: readonly string[];
}

export interface RichApprovalPresentationV3 {
  readonly title: string;
  readonly summary: string;
  readonly risk: 'low' | 'medium' | 'high' | 'critical';
  readonly authority: RichApprovalAuthorityPresentation;
}

export interface RichApprovalDecisionCapabilityV3 {
  readonly url: string;
  readonly token: string;
  readonly expiresAt: string;
}

export interface RichApprovalNotificationV3 {
  readonly protocol: 'woml.notification-provider-host';
  readonly protocolVersion: typeof RICH_NOTIFICATION_PROVIDER_PROTOCOL_VERSION;
  readonly messageType: 'deliver';
  readonly mode: 'rich-approval';
  readonly invocationId: string;
  readonly runId: string;
  readonly approvalId: string;
  readonly requestId: string;
  readonly deliveryId: string;
  readonly provider: 'slack' | 'telegram' | 'discord' | 'custom';
  readonly destination: string;
  readonly reviewDigest: string;
  readonly presentation: RichApprovalPresentationV3;
  readonly decisionCapability: RichApprovalDecisionCapabilityV3;
}

export interface RenderedRichApproval {
  readonly text: string;
  readonly blocks?: readonly unknown[];
  readonly generic: {
    readonly title: string;
    readonly summary: string;
    readonly risk: RichApprovalPresentationV3['risk'];
    readonly quorum: string;
    readonly requiredRoles: readonly string[];
    readonly reviewDigestPrefix: string;
    readonly reviewUrl: string;
  };
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function exactKeys(
  value: Record<string, unknown>,
  required: readonly string[]
): boolean {
  return Object.keys(value).length === required.length &&
    required.every(key => Object.hasOwn(value, key));
}

function bounded(value: unknown, maximum: number): string {
  const text = String(value ?? '').replace(CONTROL_OR_BIDI, '�').trim();
  const characters = [...text];
  return characters.length <= maximum
    ? text
    : `${characters.slice(0, Math.max(0, maximum - 1)).join('')}…`;
}

function objects(value: unknown): readonly Record<string, unknown>[] {
  return Array.isArray(value) ? value.filter(record) : [];
}

function summaryLines(review: JsonObject): string[] {
  const lines = [bounded(review.summary, 1_000)];
  const changes = objects(review.changes);
  if (changes.length > 0) {
    lines.push('', 'Key changes');
    for (const change of changes.slice(0, 3)) {
      lines.push(
        `• ${bounded(change.action, 16)} ${bounded(change.target, 120)} — ${bounded(change.summary, 240)}`
      );
    }
    if (changes.length > 3) lines.push(`• +${changes.length - 3} more change(s)`);
  }
  const checks = objects(review.checks);
  if (checks.length > 0) {
    lines.push('', 'Checks');
    for (const check of checks.slice(0, 3)) {
      lines.push(`• ${bounded(check.status, 16)} — ${bounded(check.name, 160)}`);
    }
    if (checks.length > 3) lines.push(`• +${checks.length - 3} more check(s)`);
  }
  const artifacts = objects(review.artifacts);
  if (artifacts.length > 0) {
    lines.push('', 'Reviewed artifacts');
    for (const artifact of artifacts.slice(0, 3)) {
      const size = Number.isSafeInteger(artifact.size) ? ` · ${artifact.size} bytes` : '';
      lines.push(`• ${bounded(artifact.name, 160)}${size}`);
    }
    if (artifacts.length > 3) lines.push(`• +${artifacts.length - 3} more artifact(s)`);
  }
  return lines.filter((line, index) => line.length > 0 || index > 0);
}

export function createRichApprovalPresentation(
  review: JsonObject,
  authority: RichApprovalAuthorityPresentation
): RichApprovalPresentationV3 {
  const risk = review.risk;
  if (!['low', 'medium', 'high', 'critical'].includes(String(risk))) {
    throw new Error('Rich approval presentation requires a valid review risk.');
  }
  const summary = bounded(summaryLines(review).join('\n'), 2_000);
  return {
    title: bounded(review.title, 160),
    summary,
    risk: risk as RichApprovalPresentationV3['risk'],
    authority: {
      required: authority.required,
      received: authority.received,
      requiredRoles: authority.requiredRoles.map(role => bounded(role, 64)),
    },
  };
}

export function assertRichApprovalNotificationV3(
  value: unknown
): asserts value is RichApprovalNotificationV3 {
  if (
    !record(value) ||
    !exactKeys(value, [
      'protocol', 'protocolVersion', 'messageType', 'mode', 'invocationId',
      'runId', 'approvalId', 'requestId', 'deliveryId', 'provider',
      'destination', 'reviewDigest', 'presentation', 'decisionCapability',
    ]) ||
    value.protocol !== 'woml.notification-provider-host' ||
    value.protocolVersion !== RICH_NOTIFICATION_PROVIDER_PROTOCOL_VERSION ||
    value.messageType !== 'deliver' ||
    value.mode !== 'rich-approval' ||
    !['slack', 'telegram', 'discord', 'custom'].includes(String(value.provider)) ||
    !['invocationId', 'runId', 'approvalId', 'requestId', 'deliveryId', 'destination']
      .every(field => typeof value[field] === 'string' && [...String(value[field])].length >= 1 && [...String(value[field])].length <= 320) ||
    !SHA256.test(String(value.reviewDigest)) ||
    !record(value.presentation) ||
    !exactKeys(value.presentation, ['title', 'summary', 'risk', 'authority']) ||
    typeof value.presentation.title !== 'string' ||
    [...value.presentation.title].length > 160 ||
    typeof value.presentation.summary !== 'string' ||
    [...value.presentation.summary].length > 2_000 ||
    !['low', 'medium', 'high', 'critical'].includes(String(value.presentation.risk)) ||
    !record(value.presentation.authority) ||
    !exactKeys(value.presentation.authority, ['required', 'received', 'requiredRoles']) ||
    !Number.isSafeInteger(value.presentation.authority.required) ||
    Number(value.presentation.authority.required) < 1 ||
    Number(value.presentation.authority.required) > 20 ||
    !Number.isSafeInteger(value.presentation.authority.received) ||
    Number(value.presentation.authority.received) < 0 ||
    Number(value.presentation.authority.received) > Number(value.presentation.authority.required) ||
    !Array.isArray(value.presentation.authority.requiredRoles) ||
    value.presentation.authority.requiredRoles.length > 16 ||
    !value.presentation.authority.requiredRoles.every(role => typeof role === 'string') ||
    !record(value.decisionCapability) ||
    !exactKeys(value.decisionCapability, ['url', 'token', 'expiresAt']) ||
    typeof value.decisionCapability.url !== 'string' ||
    !URL.canParse(value.decisionCapability.url) ||
    typeof value.decisionCapability.token !== 'string' ||
    value.decisionCapability.token.length < 32 ||
    value.decisionCapability.token.length > 4_096 ||
    typeof value.decisionCapability.expiresAt !== 'string' ||
    !Number.isFinite(Date.parse(value.decisionCapability.expiresAt))
  ) {
    throw new Error('The provider host received an invalid rich approval presentation.');
  }
}

export function renderRichApprovalNotification(
  invocation: RichApprovalNotificationV3
): RenderedRichApproval {
  assertRichApprovalNotificationV3(invocation);
  const digest = invocation.reviewDigest.slice('sha256:'.length, 'sha256:'.length + 12);
  const { presentation } = invocation;
  const quorum = `${presentation.authority.received}/${presentation.authority.required}`;
  const roles = presentation.authority.requiredRoles.length === 0
    ? 'None'
    : presentation.authority.requiredRoles.join(', ');
  const text = bounded([
    `Review required · ${presentation.risk.toUpperCase()} risk`,
    presentation.title,
    '',
    presentation.summary,
    '',
    `Quorum: ${quorum}`,
    `Required roles: ${roles}`,
    `Review digest: ${digest}…`,
    `Review and decide: ${invocation.decisionCapability.url}`,
  ].join('\n'), invocation.provider === 'discord' ? 2_000 : 4_096);
  const generic = {
    title: presentation.title,
    summary: presentation.summary,
    risk: presentation.risk,
    quorum,
    requiredRoles: presentation.authority.requiredRoles,
    reviewDigestPrefix: digest,
    reviewUrl: invocation.decisionCapability.url,
  };
  if (invocation.provider !== 'slack') return { text, generic };
  return {
    text,
    blocks: [
      {
        type: 'header',
        text: { type: 'plain_text', text: bounded(presentation.title, 150), emoji: true },
      },
      {
        type: 'section',
        text: {
          type: 'plain_text',
          text: bounded(`${presentation.risk.toUpperCase()} risk · Quorum ${quorum}\n\n${presentation.summary}`, 3_000),
          emoji: true,
        },
      },
      {
        type: 'context',
        elements: [{
          type: 'plain_text',
          text: bounded(`Roles: ${roles} · Digest: ${digest}…`, 2_000),
          emoji: true,
        }],
      },
      {
        type: 'actions',
        elements: [{
          type: 'button',
          text: { type: 'plain_text', text: 'Review and decide', emoji: true },
          url: invocation.decisionCapability.url,
          action_id: 'woml_rich_approval_review',
        }],
      },
    ],
    generic,
  };
}

export function presentationContainsForbiddenValue(
  presentation: RichApprovalPresentationV3,
  forbiddenValues: readonly string[]
): boolean {
  const serialized = JSON.stringify(presentation);
  return forbiddenValues.some(value => value.length > 0 && serialized.includes(value));
}

export function asJsonValue(value: RichApprovalPresentationV3): JsonValue {
  return value as unknown as JsonValue;
}
