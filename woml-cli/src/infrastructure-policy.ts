import { createPublicKey, verify as verifyCryptographicSignature } from 'node:crypto';
import { readFile, stat } from 'node:fs/promises';
import { resolve } from 'node:path';

export const INFRASTRUCTURE_POLICY_CONTRACT = 'woml.infrastructure-policy';
export const INFRASTRUCTURE_POLICY_CONTRACT_VERSION = 1;
export const INFRASTRUCTURE_POLICY_MAX_BYTES = 1024 * 1024;
export const INFRASTRUCTURE_POLICY_MAX_RULES = 1024;
export const INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES = 128;
export const INFRASTRUCTURE_POLICY_MAX_ROLES = 64;
export const INFRASTRUCTURE_POLICY_MAX_JSON_DEPTH = 16;
export const INFRASTRUCTURE_POLICY_MAX_JSON_VALUES = 100_000;

export type InfrastructurePolicyEffectV1 =
  | 'read'
  | 'idempotent-write'
  | 'unsafe-write';
export type InfrastructurePolicyDefaultV1 = 'allow' | 'deny';
export type InfrastructurePolicyDecisionV1 = 'allow' | 'deny';
export type InfrastructurePolicyExecutionModeV1 =
  | 'normal'
  | 'compensation'
  | 'cleanup'
  | 'recovery';

export interface InfrastructurePolicySignatureV1 {
  readonly algorithm: 'ed25519';
  readonly keyId: string;
  readonly value: string;
}

export interface InfrastructurePolicyRuleMatchV1 {
  readonly capabilities?: readonly string[];
  readonly operations?: readonly string[];
  readonly effects?: readonly InfrastructurePolicyEffectV1[];
  readonly workflows?: readonly string[];
  readonly targets?: readonly string[];
  readonly executables?: readonly string[];
  readonly argumentPrefix?: readonly string[];
  readonly executionModes?: readonly InfrastructurePolicyExecutionModeV1[];
}

export interface InfrastructurePolicyRequirementsV1 {
  readonly approval?: {
    readonly minimum: number;
    readonly roles: readonly string[];
    readonly bindReviewDigest: boolean;
    readonly maxAgeMs: number;
  };
  readonly artifacts?: {
    readonly minimum: number;
    readonly verifiedIntegrity: true;
    readonly bindReviewDigest: boolean;
  };
  readonly lock?: {
    readonly key: string;
    readonly minimumRemainingMs: number;
  };
  readonly validBetween?: {
    readonly notBefore: string;
    readonly notAfter: string;
  };
}

export interface InfrastructurePolicyRuleV1 {
  readonly id: string;
  readonly description?: string;
  readonly match: InfrastructurePolicyRuleMatchV1;
  readonly decision: InfrastructurePolicyDecisionV1;
  readonly requires?: InfrastructurePolicyRequirementsV1;
}

export interface InfrastructurePolicyBundleV1 {
  readonly contract: typeof INFRASTRUCTURE_POLICY_CONTRACT;
  readonly contractVersion: typeof INFRASTRUCTURE_POLICY_CONTRACT_VERSION;
  readonly id: string;
  readonly version: string;
  readonly environment: string;
  readonly defaults: Readonly<
    Record<InfrastructurePolicyEffectV1, InfrastructurePolicyDefaultV1>
  >;
  readonly rules: readonly InfrastructurePolicyRuleV1[];
  readonly signature?: InfrastructurePolicySignatureV1;
}

export interface PreparedInfrastructurePolicyV1 {
  readonly bundle: InfrastructurePolicyBundleV1;
  readonly digest: string;
  readonly canonicalPayload: Uint8Array;
  readonly signatureStatus: 'unsigned' | 'verified';
  readonly signingKeyId?: string;
}

export interface InfrastructurePolicyTrustV1 {
  readonly keys?: Readonly<Record<string, string>>;
  readonly revokedKeyIds?: readonly string[];
  readonly requireSignature?: boolean;
}

export class InfrastructurePolicyError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(`${message} Corrective action: provide a valid, operator-owned Infrastructure Policy v1 bundle.`);
    this.name = 'InfrastructurePolicyError';
    this.code = code;
  }
}

const identifierPattern = /^[a-z][a-z0-9._-]{0,127}$/;
const versionPattern = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
const rolePattern = /^[a-z][A-Za-z0-9:_-]{0,127}$/;
const ed25519SpkiPrefix = Buffer.from('302a300506032b6570032100', 'hex');

export async function loadInfrastructurePolicy(
  path: string,
  trust: InfrastructurePolicyTrustV1 = {}
): Promise<PreparedInfrastructurePolicyV1> {
  const policyPath = resolve(path);
  let source: string;
  try {
    const metadata = await stat(policyPath);
    if (!metadata.isFile() || metadata.size > INFRASTRUCTURE_POLICY_MAX_BYTES) {
      throw new InfrastructurePolicyError(
        'WOML_POLICY_INVALID',
        'The infrastructure policy must be a JSON file no larger than 1 MiB.'
      );
    }
    source = await readFile(policyPath, 'utf8');
  } catch (error) {
    if (error instanceof InfrastructurePolicyError) throw error;
    throw new InfrastructurePolicyError(
      'WOML_POLICY_UNAVAILABLE',
      `The infrastructure policy could not be read: ${policyPath}`
    );
  }
  return parseInfrastructurePolicy(source, trust);
}

export function parseInfrastructurePolicy(
  source: string,
  trust: InfrastructurePolicyTrustV1 = {}
): PreparedInfrastructurePolicyV1 {
  if (Buffer.byteLength(source) > INFRASTRUCTURE_POLICY_MAX_BYTES) {
    invalid('The infrastructure policy exceeds the 1 MiB limit.');
  }
  let decoded: unknown;
  try {
    decoded = new StrictJsonParser(source).parse();
  } catch (error) {
    if (error instanceof InfrastructurePolicyError) throw error;
    invalid('The infrastructure policy is not strict JSON.');
  }
  return prepareInfrastructurePolicy(decoded, trust);
}

export function prepareInfrastructurePolicy(
  input: unknown,
  trust: InfrastructurePolicyTrustV1 = {}
): PreparedInfrastructurePolicyV1 {
  const bundle = normalizeBundle(input);
  const canonicalPayload = Buffer.from(
    canonicalizeInfrastructurePolicy(bundle),
    'utf8'
  );
  const digest = `sha256:${new Bun.CryptoHasher('sha256')
    .update(canonicalPayload)
    .digest('hex')}`;
  const revoked = new Set(trust.revokedKeyIds ?? []);
  let signatureStatus: PreparedInfrastructurePolicyV1['signatureStatus'] =
    'unsigned';
  let signingKeyId: string | undefined;
  if (bundle.signature === undefined) {
    if (trust.requireSignature === true) {
      signatureInvalid('The infrastructure policy requires a signature.');
    }
  } else {
    signingKeyId = bundle.signature.keyId;
    if (revoked.has(signingKeyId)) {
      signatureInvalid(`Infrastructure policy signing key "${signingKeyId}" is revoked.`);
    }
    const encodedKey = trust.keys?.[signingKeyId];
    if (encodedKey === undefined) {
      signatureInvalid(`Infrastructure policy signing key "${signingKeyId}" is not trusted.`);
    }
    const publicKey = decodeBase64Url(encodedKey, 32, 'public key');
    const signature = decodeBase64Url(bundle.signature.value, 64, 'signature');
    let verified = false;
    try {
      verified = verifyCryptographicSignature(
        null,
        canonicalPayload,
        createPublicKey({
          key: Buffer.concat([ed25519SpkiPrefix, publicKey]),
          format: 'der',
          type: 'spki',
        }),
        signature
      );
    } catch {
      verified = false;
    }
    if (!verified) signatureInvalid('The infrastructure policy signature is invalid.');
    signatureStatus = 'verified';
  }
  return {
    bundle,
    digest,
    canonicalPayload,
    signatureStatus,
    ...(signingKeyId === undefined ? {} : { signingKeyId }),
  };
}

/** Canonical Policy v1 payload. The detached signature never signs itself. */
export function canonicalizeInfrastructurePolicy(
  bundle: InfrastructurePolicyBundleV1
): string {
  const { signature: _signature, ...payload } = bundle;
  return canonicalJson(payload);
}

function normalizeBundle(input: unknown): InfrastructurePolicyBundleV1 {
  const authored = strictObject(
    input,
    'policy',
    [
      'contract',
      'contractVersion',
      'id',
      'version',
      'environment',
      'defaults',
      'rules',
      'signature',
    ],
    ['contract', 'contractVersion', 'id', 'version', 'environment', 'defaults', 'rules']
  );
  if (
    authored.contract !== INFRASTRUCTURE_POLICY_CONTRACT ||
    authored.contractVersion !== INFRASTRUCTURE_POLICY_CONTRACT_VERSION
  ) {
    invalid('The document does not match Infrastructure Policy v1.');
  }
  const id = identifier(authored.id, 'policy.id');
  const version = patternString(authored.version, 'policy.version', versionPattern);
  const environment = identifier(authored.environment, 'policy.environment');
  const defaultsValue = strictObject(
    authored.defaults,
    'policy.defaults',
    ['read', 'idempotent-write', 'unsafe-write'],
    ['read', 'idempotent-write', 'unsafe-write']
  );
  const defaults = {
    read: enumValue(defaultsValue.read, ['allow', 'deny'], 'defaults.read'),
    'idempotent-write': enumValue(
      defaultsValue['idempotent-write'],
      ['allow', 'deny'],
      'defaults.idempotent-write'
    ),
    'unsafe-write': enumValue(
      defaultsValue['unsafe-write'],
      ['allow', 'deny'],
      'defaults.unsafe-write'
    ),
  } satisfies InfrastructurePolicyBundleV1['defaults'];
  if (
    !Array.isArray(authored.rules) ||
    authored.rules.length > INFRASTRUCTURE_POLICY_MAX_RULES
  ) {
    invalid(`policy.rules must contain at most ${INFRASTRUCTURE_POLICY_MAX_RULES} rules.`);
  }
  const ruleIds = new Set<string>();
  const rules = authored.rules.map((rule, index) => {
    const normalized = normalizeRule(rule, index);
    if (ruleIds.has(normalized.id)) invalid(`Duplicate policy rule id "${normalized.id}".`);
    ruleIds.add(normalized.id);
    return normalized;
  });
  const signature =
    authored.signature === undefined
      ? undefined
      : normalizeSignature(authored.signature);
  return {
    contract: INFRASTRUCTURE_POLICY_CONTRACT,
    contractVersion: INFRASTRUCTURE_POLICY_CONTRACT_VERSION,
    id,
    version,
    environment,
    defaults,
    rules,
    ...(signature === undefined ? {} : { signature }),
  };
}

function normalizeRule(input: unknown, index: number): InfrastructurePolicyRuleV1 {
  const label = `policy.rules[${index}]`;
  const authored = strictObject(
    input,
    label,
    ['id', 'description', 'match', 'decision', 'requires'],
    ['id', 'match', 'decision']
  );
  const id = identifier(authored.id, `${label}.id`);
  const description =
    authored.description === undefined
      ? undefined
      : boundedString(authored.description, `${label}.description`, 512);
  const match = normalizeMatch(authored.match, `${label}.match`);
  const decision = enumValue(
    authored.decision,
    ['allow', 'deny'],
    `${label}.decision`
  );
  const requirements =
    authored.requires === undefined
      ? undefined
      : normalizeRequirements(authored.requires, `${label}.requires`);
  if (decision === 'deny' && requirements !== undefined) {
    invalid(`${label} cannot attach requirements to a deny decision.`);
  }
  return {
    id,
    ...(description === undefined ? {} : { description }),
    match,
    decision,
    ...(requirements === undefined ? {} : { requires: requirements }),
  };
}

function normalizeMatch(
  input: unknown,
  label: string
): InfrastructurePolicyRuleMatchV1 {
  const authored = strictObject(
    input,
    label,
    [
      'capabilities',
      'operations',
      'effects',
      'workflows',
      'targets',
      'executables',
      'argumentPrefix',
      'executionModes',
    ],
    []
  );
  if (Object.keys(authored).length === 0) {
    invalid(`${label} must contain at least one bounded matcher.`);
  }
  return {
    ...(authored.capabilities === undefined
      ? {}
      : { capabilities: identifierSet(authored.capabilities, `${label}.capabilities`) }),
    ...(authored.operations === undefined
      ? {}
      : { operations: identifierSet(authored.operations, `${label}.operations`) }),
    ...(authored.effects === undefined
      ? {}
      : {
          effects: enumSet(
            authored.effects,
            ['read', 'idempotent-write', 'unsafe-write'],
            `${label}.effects`
          ),
        }),
    ...(authored.workflows === undefined
      ? {}
      : { workflows: identifierSet(authored.workflows, `${label}.workflows`) }),
    ...(authored.targets === undefined
      ? {}
      : { targets: identifierSet(authored.targets, `${label}.targets`) }),
    ...(authored.executables === undefined
      ? {}
      : { executables: identifierSet(authored.executables, `${label}.executables`) }),
    ...(authored.argumentPrefix === undefined
      ? {}
      : { argumentPrefix: argumentList(authored.argumentPrefix, `${label}.argumentPrefix`) }),
    ...(authored.executionModes === undefined
      ? {}
      : {
          executionModes: enumSet(
            authored.executionModes,
            ['normal', 'compensation', 'cleanup', 'recovery'],
            `${label}.executionModes`
          ),
        }),
  };
}

function normalizeRequirements(
  input: unknown,
  label: string
): InfrastructurePolicyRequirementsV1 {
  const authored = strictObject(
    input,
    label,
    ['approval', 'artifacts', 'lock', 'validBetween'],
    []
  );
  if (Object.keys(authored).length === 0) invalid(`${label} cannot be empty.`);
  const approval = authored.approval === undefined
    ? undefined
    : (() => {
        const value = strictObject(
          authored.approval,
          `${label}.approval`,
          ['minimum', 'roles', 'bindReviewDigest', 'maxAgeMs'],
          ['minimum', 'roles', 'bindReviewDigest', 'maxAgeMs']
        );
        if (typeof value.bindReviewDigest !== 'boolean') {
          invalid(`${label}.approval.bindReviewDigest must be boolean.`);
        }
        return {
          minimum: integer(value.minimum, 1, 20, `${label}.approval.minimum`),
          roles: roleSet(value.roles, `${label}.approval.roles`),
          bindReviewDigest: value.bindReviewDigest,
          maxAgeMs: integer(
            value.maxAgeMs,
            1,
            365 * 24 * 60 * 60 * 1000,
            `${label}.approval.maxAgeMs`
          ),
        };
      })();
  const artifacts = authored.artifacts === undefined
    ? undefined
    : (() => {
        const value = strictObject(
          authored.artifacts,
          `${label}.artifacts`,
          ['minimum', 'verifiedIntegrity', 'bindReviewDigest'],
          ['minimum', 'verifiedIntegrity', 'bindReviewDigest']
        );
        if (value.verifiedIntegrity !== true || typeof value.bindReviewDigest !== 'boolean') {
          invalid(`${label}.artifacts requires verifiedIntegrity=true and bindReviewDigest boolean.`);
        }
        return {
          minimum: integer(value.minimum, 1, 32, `${label}.artifacts.minimum`),
          verifiedIntegrity: true as const,
          bindReviewDigest: value.bindReviewDigest,
        };
      })();
  const lock = authored.lock === undefined
    ? undefined
    : (() => {
        const value = strictObject(
          authored.lock,
          `${label}.lock`,
          ['key', 'minimumRemainingMs'],
          ['key', 'minimumRemainingMs']
        );
        return {
          key: boundedString(value.key, `${label}.lock.key`, 256),
          minimumRemainingMs: integer(
            value.minimumRemainingMs,
            1,
            86_400_000,
            `${label}.lock.minimumRemainingMs`
          ),
        };
      })();
  const validBetween = authored.validBetween === undefined
    ? undefined
    : (() => {
        const value = strictObject(
          authored.validBetween,
          `${label}.validBetween`,
          ['notBefore', 'notAfter'],
          ['notBefore', 'notAfter']
        );
        const notBefore = timestamp(value.notBefore, `${label}.validBetween.notBefore`);
        const notAfter = timestamp(value.notAfter, `${label}.validBetween.notAfter`);
        if (Date.parse(notBefore) >= Date.parse(notAfter)) {
          invalid(`${label}.validBetween must end after it starts.`);
        }
        return { notBefore, notAfter };
      })();
  return {
    ...(approval === undefined ? {} : { approval }),
    ...(artifacts === undefined ? {} : { artifacts }),
    ...(lock === undefined ? {} : { lock }),
    ...(validBetween === undefined ? {} : { validBetween }),
  };
}

function normalizeSignature(input: unknown): InfrastructurePolicySignatureV1 {
  const value = strictObject(
    input,
    'policy.signature',
    ['algorithm', 'keyId', 'value'],
    ['algorithm', 'keyId', 'value']
  );
  if (value.algorithm !== 'ed25519') {
    signatureInvalid('Infrastructure Policy v1 supports only Ed25519 signatures.');
  }
  return {
    algorithm: 'ed25519',
    keyId: identifier(value.keyId, 'policy.signature.keyId'),
    value: boundedString(value.value, 'policy.signature.value', 128),
  };
}

function canonicalJson(value: unknown): string {
  if (value === null) return 'null';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) invalid('Policy numbers must be safe integers.');
    return JSON.stringify(value);
  }
  if (typeof value === 'string') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record)
      .sort()
      .map(key => `${JSON.stringify(key)}:${canonicalJson(record[key])}`)
      .join(',')}}`;
  }
  invalid('The policy contains an unsupported JSON value.');
}

function strictObject(
  input: unknown,
  label: string,
  allowed: readonly string[],
  required: readonly string[]
): Record<string, unknown> {
  if (input === null || typeof input !== 'object' || Array.isArray(input)) {
    invalid(`${label} must be an object.`);
  }
  const object = input as Record<string, unknown>;
  if (Object.keys(object).some(key => !allowed.includes(key))) {
    invalid(`${label} contains an unknown field.`);
  }
  if (required.some(key => !(key in object))) {
    invalid(`${label} is missing a required field.`);
  }
  return object;
}

function identifier(value: unknown, label: string): string {
  return patternString(value, label, identifierPattern);
}

function patternString(value: unknown, label: string, pattern: RegExp): string {
  const text = boundedString(value, label, 128);
  if (!pattern.test(text)) invalid(`${label} has an invalid format.`);
  return text;
}

function boundedString(value: unknown, label: string, maximumBytes: number): string {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    Buffer.byteLength(value) > maximumBytes ||
    value.includes('\0') ||
    !isWellFormedUnicode(value)
  ) {
    invalid(`${label} must be a non-empty string no larger than ${maximumBytes} bytes.`);
  }
  return value;
}

function identifierSet(value: unknown, label: string): readonly string[] {
  return stringSet(value, label, identifierPattern, INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES);
}

function roleSet(value: unknown, label: string): readonly string[] {
  return stringSet(value, label, rolePattern, INFRASTRUCTURE_POLICY_MAX_ROLES, true);
}

function stringSet(
  value: unknown,
  label: string,
  pattern: RegExp,
  maximum: number,
  allowEmpty = false
): readonly string[] {
  if (!Array.isArray(value) || (!allowEmpty && value.length === 0) || value.length > maximum) {
    invalid(`${label} must contain ${allowEmpty ? 'zero' : 'one'} through ${maximum} values.`);
  }
  const values = value.map((entry, index) =>
    patternString(entry, `${label}[${index}]`, pattern)
  );
  if (new Set(values).size !== values.length) invalid(`${label} cannot contain duplicates.`);
  return values;
}

function enumSet<const T extends string>(
  value: unknown,
  allowed: readonly T[],
  label: string
): readonly T[] {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    value.length > INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES
  ) {
    invalid(`${label} must be a non-empty bounded array.`);
  }
  const values = value.map((entry, index) =>
    enumValue(entry, allowed, `${label}[${index}]`)
  );
  if (new Set(values).size !== values.length) invalid(`${label} cannot contain duplicates.`);
  return values;
}

function argumentList(value: unknown, label: string): readonly string[] {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    value.length > INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES
  ) {
    invalid(`${label} must contain 1 through ${INFRASTRUCTURE_POLICY_MAX_MATCH_VALUES} arguments.`);
  }
  return value.map((entry, index) => boundedString(entry, `${label}[${index}]`, 4096));
}

function enumValue<const T extends string>(
  value: unknown,
  allowed: readonly T[],
  label: string
): T {
  if (typeof value !== 'string' || !allowed.includes(value as T)) {
    invalid(`${label} must be one of: ${allowed.join(', ')}.`);
  }
  return value as T;
}

function integer(value: unknown, minimum: number, maximum: number, label: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < minimum || (value as number) > maximum) {
    invalid(`${label} must be an integer from ${minimum} through ${maximum}.`);
  }
  return value as number;
}

function timestamp(value: unknown, label: string): string {
  const text = boundedString(value, label, 64);
  if (
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?Z$/.test(text) ||
    Number.isNaN(Date.parse(text))
  ) {
    invalid(`${label} must be an RFC 3339 UTC timestamp.`);
  }
  return text;
}

function decodeBase64Url(value: string, length: number, label: string): Buffer {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) signatureInvalid(`The ${label} is not canonical base64url.`);
  const decoded = Buffer.from(value, 'base64url');
  if (decoded.length !== length || decoded.toString('base64url') !== value) {
    signatureInvalid(`The ${label} has an invalid length or encoding.`);
  }
  return decoded;
}

function invalid(message: string): never {
  throw new InfrastructurePolicyError('WOML_POLICY_INVALID', message);
}

function signatureInvalid(message: string): never {
  throw new InfrastructurePolicyError('WOML_POLICY_SIGNATURE_INVALID', message);
}

/** A bounded strict JSON reader used to reject duplicate keys before signing. */
class StrictJsonParser {
  private position = 0;
  private values = 0;

  constructor(private readonly source: string) {}

  parse(): unknown {
    this.skipWhitespace();
    const value = this.parseValue(0);
    this.skipWhitespace();
    if (this.position !== this.source.length) this.fail();
    return value;
  }

  private parseValue(depth: number): unknown {
    this.values += 1;
    if (
      depth > INFRASTRUCTURE_POLICY_MAX_JSON_DEPTH ||
      this.values > INFRASTRUCTURE_POLICY_MAX_JSON_VALUES
    ) {
      invalid('The infrastructure policy exceeds its JSON complexity limit.');
    }
    const character = this.source[this.position];
    if (character === '{') return this.parseObject(depth + 1);
    if (character === '[') return this.parseArray(depth + 1);
    if (character === '"') return this.parseString();
    if (character === 't') return this.literal('true', true);
    if (character === 'f') return this.literal('false', false);
    if (character === 'n') return this.literal('null', null);
    return this.parseNumber();
  }

  private parseObject(depth: number): Record<string, unknown> {
    this.position += 1;
    this.skipWhitespace();
    const object: Record<string, unknown> = Object.create(null) as Record<string, unknown>;
    const keys = new Set<string>();
    if (this.source[this.position] === '}') {
      this.position += 1;
      return object;
    }
    while (true) {
      if (this.source[this.position] !== '"') this.fail();
      const key = this.parseString();
      if (keys.has(key)) invalid(`The infrastructure policy contains duplicate JSON key "${key}".`);
      keys.add(key);
      this.skipWhitespace();
      if (this.source[this.position] !== ':') this.fail();
      this.position += 1;
      this.skipWhitespace();
      object[key] = this.parseValue(depth);
      this.skipWhitespace();
      const separator = this.source[this.position];
      if (separator === '}') {
        this.position += 1;
        return object;
      }
      if (separator !== ',') this.fail();
      this.position += 1;
      this.skipWhitespace();
    }
  }

  private parseArray(depth: number): unknown[] {
    this.position += 1;
    this.skipWhitespace();
    const values: unknown[] = [];
    if (this.source[this.position] === ']') {
      this.position += 1;
      return values;
    }
    while (true) {
      values.push(this.parseValue(depth));
      this.skipWhitespace();
      const separator = this.source[this.position];
      if (separator === ']') {
        this.position += 1;
        return values;
      }
      if (separator !== ',') this.fail();
      this.position += 1;
      this.skipWhitespace();
    }
  }

  private parseString(): string {
    const start = this.position;
    this.position += 1;
    while (this.position < this.source.length) {
      const code = this.source.charCodeAt(this.position);
      if (code === 0x22) {
        this.position += 1;
        try {
          const value = JSON.parse(this.source.slice(start, this.position)) as string;
          if (!isWellFormedUnicode(value)) this.fail();
          return value;
        } catch {
          this.fail();
        }
      }
      if (code < 0x20) this.fail();
      if (code === 0x5c) {
        this.position += 1;
        const escape = this.source[this.position];
        if (escape === 'u') {
          const hex = this.source.slice(this.position + 1, this.position + 5);
          if (!/^[0-9A-Fa-f]{4}$/.test(hex)) this.fail();
          this.position += 5;
          continue;
        }
        if (!'"\\/bfnrt'.includes(escape ?? '')) this.fail();
      }
      this.position += 1;
    }
    this.fail();
  }

  private parseNumber(): number {
    const match = this.source
      .slice(this.position)
      .match(/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/);
    if (match === null) this.fail();
    this.position += match[0].length;
    const value = Number(match[0]);
    if (!Number.isFinite(value)) this.fail();
    return value;
  }

  private literal<T>(source: string, value: T): T {
    if (!this.source.startsWith(source, this.position)) this.fail();
    this.position += source.length;
    return value;
  }

  private skipWhitespace(): void {
    while (
      this.source[this.position] === ' ' ||
      this.source[this.position] === '\t' ||
      this.source[this.position] === '\r' ||
      this.source[this.position] === '\n'
    ) {
      this.position += 1;
    }
  }

  private fail(): never {
    invalid(`The infrastructure policy contains invalid JSON near byte ${this.position}.`);
  }
}

function isWellFormedUnicode(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}
