# WOML Durable Artifacts v1

**Status:** Frozen by DA0 and DA1 on 2026-10-11  
**Capability:** `artifacts`  
**Operations:** `put`, `head`, `materialize`  
**Transport:** Existing Capability Call v1  
**Authority:** Rust core

## Purpose

Durable Artifacts v1 preserves immutable files produced or consumed by one
workflow run. The bytes live outside workflow context and event history. WOML
stores only a bounded, versioned reference containing identity and integrity
metadata.

Artifacts are not `services.storage` objects. Storage is mutable,
author-keyed application data. An artifact is engine-identified evidence with
run, step, attempt, and operation provenance. An artifact can never be updated,
renamed, or deleted by workflow code.

## Frozen author API

```ts
interface WomlArtifactPutOptions {
  readonly name: string;
  readonly contentType?: string;
  readonly retention?: string;
}

interface WomlArtifactReferenceV1 {
  readonly contract: "woml.artifact-reference";
  readonly contractVersion: 1;
  readonly id: string;
  readonly name: string;
  readonly size: number;
  readonly sha256: string;
  readonly contentType: string;
}

interface WomlArtifactHeadV1 {
  readonly artifact: WomlArtifactReferenceV1;
  readonly createdAt: string;
  readonly expiresAt: string;
  readonly integrity: "verified";
}

interface WomlArtifactsService {
  readonly put: (
    path: string,
    options: WomlArtifactPutOptions
  ) => Promise<WomlArtifactReferenceV1>;
  readonly head: (
    reference: WomlArtifactReferenceV1
  ) => Promise<WomlArtifactHeadV1>;
  readonly materialize: (
    reference: WomlArtifactReferenceV1
  ) => Promise<string>;
}
```

Author retention uses a whole positive duration matching
`^[1-9][0-9]*(h|d|w)$`. Bun converts it to milliseconds before the capability
boundary; Rust independently checks the normalized value against active
policy. Fractional, zero, negative, mixed-unit, and millisecond strings are
invalid in v1.

`put` requires a stable artifact `name`. That name is also the managed
operation name for v1. Two `put` calls with the same name in one step identify
the same logical operation; different input under that identity fails with
`WOML_ARTIFACT_OPERATION_IDENTITY_CONFLICT`. This keeps the common API at two
arguments without hiding retry identity in a third options object.

`head` and `materialize` are read-only. They may use automatic Capability Call
identities and are safe to repeat after an uncommitted interruption.

## Normalized wire requests

The Bun facade parses the retention duration and emits one of these values as
the Capability Call input:

```json
{
  "contract": "woml.artifacts",
  "contractVersion": 1,
  "kind": "request",
  "operation": "put",
  "input": {
    "sourcePath": "infrastructure/release.tfplan",
    "name": "production-terraform-plan",
    "contentType": "application/octet-stream",
    "retentionMs": 2592000000
  }
}
```

```json
{
  "contract": "woml.artifacts",
  "contractVersion": 1,
  "kind": "request",
  "operation": "head",
  "input": {
    "artifact": {
      "contract": "woml.artifact-reference",
      "contractVersion": 1,
      "id": "artifact_01k7qh7s00a1b2c3d4e5f6g7h8",
      "name": "production-terraform-plan",
      "size": 184320,
      "sha256": "c742b8669e07a0ca8a2a8d79b22b17504c51f7b0d81623a8045fece436f02942",
      "contentType": "application/octet-stream"
    }
  }
}
```

`materialize` uses the same reference input and changes only `operation`.

## Identity

An artifact ID identifies one logical artifact record and its provenance. It
is not derived from the body and is not authorization. SHA-256 identifies the
exact bytes. Multiple logical artifacts may share one physical blob when their
bytes match, but they retain separate IDs, provenance, retention, and access
records.

Artifact IDs use the frozen form `artifact_<26 lowercase Crockford base32
characters>`. The content digest is exactly 64 lowercase hexadecimal
characters without a `sha256:` prefix in the public reference.

## Access boundary

Artifact access is same-run in v1:

- ordinary steps of the producing run can use the reference;
- lifecycle hooks for that same run can use it;
- a reference copied into another run is not authority and is rejected;
- parent/child workflow grants are not inferred by scanning JSON payloads;
- operator inspection can see bounded metadata but cannot read the body;
- operator export is deferred until DA9 and must be explicit and audited.

Workflow-call artifact grants are a later versioned extension. Rich Plan
Approvals may bind a decision to the artifact ID and digest later without
changing Artifact Reference v1.

## Source path policy

Author paths use forward-slash, project-relative syntax. Absolute paths,
Windows drive paths, backslashes, NUL/control characters, empty segments,
`.`/`..` segments, and paths longer than 4,096 UTF-8 bytes are rejected before
filesystem access.

Rust resolves the path beneath a canonical project root and one of the
operator-approved read roots. It accepts only a regular file, rejects path
escape through links or Windows reparse points, and verifies that the opened
file did not change while it was read.

## Frozen defaults and limits

Protocol maxima and operator defaults are separate. Policy can lower a limit
but cannot exceed the protocol ceiling.

| Limit | Default policy | Protocol ceiling |
| --- | ---: | ---: |
| One artifact | 1 GiB | 1 TiB |
| Artifacts created by one run | 10 GiB | 4 TiB |
| Total managed artifact bytes | 100 GiB | 1 PiB |
| Concurrent transfers | 4 | 64 |
| Transfer timeout | 1 hour | 24 hours |
| Retention | 30 days | 365 days |
| Minimum requested retention | 1 hour | — |
| Artifact name | — | 256 UTF-8 bytes |
| Content type | — | 255 ASCII characters |

All byte counters remain below JavaScript's maximum safe integer. Quota
failure never evicts another artifact.

## Materialization lifetime

Materialized files are private to one step or lifecycle attempt. Rust streams
to a private staging file, verifies size and SHA-256, then exposes a path to the
current Bun worker. The path is removed when the attempt succeeds, fails,
times out, is cancelled, or is recovered after a crash.

A materialized path is not a durable result. Authors return the artifact
reference to later steps and materialize it again where it is consumed.

## Retention

The requested duration is clamped only by rejection: a value outside the
active policy fails rather than silently changing. The committed artifact
stores the effective expiration time.

Expiration makes an artifact eligible, not immediately deletable. Active
runs, unresolved approvals, active maintenance, backup snapshots, and any
future explicit grant retain a hold. Physical bytes are deleted only when no
logical artifact record remains reachable.

Workflow code cannot delete an artifact. `woml prune` and maintenance are the
only deletion authorities.

## Secret handling

Artifact bodies never enter events, context, progress, logs, metrics, or
ordinary diagnostics. Known resolved WOML secret byte sequences of at least
eight bytes are scanned during `put`. Artifact Policy v1 selects `reject`,
`quarantine`, or `allow`; the default is `reject`.

This is not data-loss-prevention magic. Encoded, encrypted, compressed,
transformed, split, derived, unknown, or short secrets may not be recognized.
The local backend is not transparent encryption at rest. Operators protect it
with a least-privilege account, filesystem permissions, encrypted disks, and
encrypted backups.

## Backend contract

Local and S3-compatible backends expose the same semantic operations:

- stage a streamed blob;
- finalize it conditionally by digest;
- open a verified stream;
- inspect existence and size;
- delete an unreachable blob;
- abort and collect staging data;
- report health without returning credentials or private locations.

SQLite remains authoritative for logical artifact identity, provenance,
access, settlement, and retention. A backend object is never sufficient proof
that a logical artifact exists.

## Atomicity and recovery

`put` becomes successful only after Rust durably settles the logical operation.
An interruption before settlement exposes no reference. Staged or physically
published but unreachable data is collected later. If settlement committed,
duplicate delivery returns the original reference. If the runtime cannot
prove which boundary was crossed, it fails closed with
`WOML_ARTIFACT_INTERRUPTED` and does not invent success.

`materialize` is read-only and may be repeated after deleting its private
partial file. Digest disagreement fails with `WOML_ARTIFACT_CORRUPT`; the
runtime never exposes the file.

## Frozen failure codes

| Code | Meaning |
| --- | --- |
| `WOML_ARTIFACT_SERVICE_DISABLED` | Runtime policy does not enable artifacts |
| `WOML_ARTIFACT_PATH_INVALID` | Source path syntax is invalid |
| `WOML_ARTIFACT_SOURCE_DENIED` | Source is outside approved roots |
| `WOML_ARTIFACT_SOURCE_CHANGED` | File identity or bytes changed during import |
| `WOML_ARTIFACT_SOURCE_TYPE_UNSUPPORTED` | Source is not an approved regular file |
| `WOML_ARTIFACT_TOO_LARGE` | Artifact exceeds its byte ceiling |
| `WOML_ARTIFACT_QUOTA_EXCEEDED` | Run or store quota would be exceeded |
| `WOML_ARTIFACT_RETENTION_INVALID` | Requested retention violates policy |
| `WOML_ARTIFACT_NOT_FOUND` | Logical artifact does not exist |
| `WOML_ARTIFACT_ACCESS_DENIED` | Current run is not authorized |
| `WOML_ARTIFACT_EXPIRED` | Artifact expired and is no longer readable |
| `WOML_ARTIFACT_QUARANTINED` | Policy quarantined the body |
| `WOML_ARTIFACT_CORRUPT` | Size or digest verification failed |
| `WOML_ARTIFACT_BACKEND_UNAVAILABLE` | Configured byte backend cannot serve the operation |
| `WOML_ARTIFACT_TIMED_OUT` | Transfer exceeded its deadline |
| `WOML_ARTIFACT_CANCELLED` | Run cancellation stopped the transfer |
| `WOML_ARTIFACT_MATERIALIZATION_UNAVAILABLE` | Private materialization could not be created |
| `WOML_ARTIFACT_OPERATION_IDENTITY_CONFLICT` | Stable operation name was reused with different input |
| `WOML_ARTIFACT_INTERRUPTED` | An interrupted write cannot be proven successful or absent |

## Existing protocol proof

Capability Call v1 already supports capability `artifacts`, static operations,
versioned input/result values, operation identity, byte/time limits, correlated
out-of-order replies, and generic failures. No generic protocol field changes.
DA1 fixtures validate the complete Artifact v1 payload inside the unchanged
Capability Call v1 schema.

## DA7 and DA8 operational contracts

- [Artifact Maintenance v1](../schemas/artifact-maintenance.v1.schema.json)
  freezes body-free retention, integrity, orphan-collection, and live-store
  measurements returned by `woml prune --json`.
- [Backup Manifest v2](../schemas/backup-manifest.v2.schema.json) binds a
  coherent SQLite snapshot to a fully verified content-addressed artifact
  inventory while preserving Backup Manifest v1 for stores without artifacts.
- [Artifact Backend v1](../schemas/artifact-backend.v1.schema.json) keeps local
  and S3-compatible storage behind identical workflow semantics. S3 credential
  values are resolved from the secret authority and never persisted in policy,
  metadata, events, backup manifests, or diagnostics.

## DA9 operator and telemetry contracts

- [Artifact Inspection v1](../schemas/artifact-inspection.v1.schema.json)
  exposes bounded name, size, digest prefix, state, retention, and provenance.
  [Artifact Run Inspection v1](../schemas/artifact-run-inspection.v1.schema.json)
  limits one run view to 100 records and declares truncation explicitly.
- [Artifact Export v1](../schemas/artifact-export.v1.schema.json) is an
  authenticated local operator action. Rust verifies the bytes, refuses an
  existing destination, and records an audit ID. Its response and audit row
  deliberately omit the destination path.
- [Artifact Observation v1](../schemas/artifact-observation.v1.schema.json)
  summarizes transfer count, bytes, duration, failures, corruption, orphans,
  live usage, and backend health without per-artifact identifiers.
- [Runtime Metrics v2](../schemas/runtime-metrics.v2.schema.json) is an
  additive artifact-only metric family emitted alongside frozen Runtime
  Metrics v1. The only label is `backend=local|s3`.

Operator inspection and export never create public or signed URLs. Artifact
bodies, body-derived values, full digests, paths, endpoints, object keys, and
credentials remain forbidden from logs and telemetry.

## Frozen DA0 decisions

1. `services.artifacts` is separate from mutable `services.storage`.
2. V1 has only `put`, `head`, and `materialize`.
3. Put's required artifact name is also its stable operation identity.
4. References contain bounded immutable metadata and never body bytes.
5. Artifact ID, body digest, and backend object key are distinct identities.
6. Access is same-run; lifecycle hooks share their run's authority.
7. Workflow-call sharing and public download URLs are excluded.
8. Source paths are relative, canonicalized, and Rust-authorized.
9. Materializations are attempt-scoped and never durable workflow output.
10. Physical deduplication never merges logical provenance.
11. Retention requests outside policy fail rather than clamp silently.
12. Workflow code cannot mutate or delete artifacts.
13. Known-secret scanning is bounded and explicitly imperfect.
14. Local storage relies on OS/disk protection rather than claiming encryption.
15. Local and S3-compatible backends implement one semantic contract.
16. Approval binding consumes Artifact Reference v1 later; it is not part of DA1.

