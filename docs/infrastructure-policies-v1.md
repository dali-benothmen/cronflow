# Infrastructure Policies v1: Boundary and Threat Model

**Status:** Frozen through IP3

**Contract:** `woml.infrastructure-policy` v1  

**Enforcement:** Planned for IP4; descriptor construction and evaluation do not yet authorize or execute effects

## Purpose

Infrastructure Policies are an operator-owned authorization layer for protected WOML effects. They answer whether a validated operation may execute in a configured environment and whether engine-owned evidence is required first.

They do not replace:

- workflow control flow;
- `<config>` scheduling and concurrency policies;
- Process Policy, Artifact Policy, or other capability-specific limits;
- operating-system permissions, cloud IAM, Kubernetes admission, or database permissions.

An infrastructure-policy allow can never expand authority granted by those lower layers.

## Trust Boundary

Trusted inputs are produced or verified by the Rust runtime:

- policy bundle bytes, canonical digest, and verified signing key;
- workflow definition hash and stable run/attempt identity;
- capability and operation registered by the Rust handler;
- handler-normalized safe operation metadata;
- runtime-selected environment and target;
- durable approval, artifact, lock, and recovery records;
- runtime authority time.

Untrusted inputs include:

- workflow payloads and trigger bodies;
- JavaScript and TypeScript return values;
- module exports;
- AI output and generated commands;
- custom-provider payloads and callbacks;
- process stdout and stderr;
- HTTP and database responses;
- arbitrary artifact metadata authored by a script.

Untrusted values may be part of an operation, but cannot assert that an approval exists, an artifact is verified, a lease is owned, a reviewer has a role, a target is production, or a policy permits an effect.

## Frozen Semantics

1. Policy is declarative JSON and cannot execute code or perform I/O.
2. Rule evaluation will occur in Rust after request validation and before any operation-started event or handler invocation.
3. Deny overrides allow. All requirements on a matching allow must be satisfied.
4. With enforcement active, an unmatched unsafe write is denied by default.
5. Read and idempotent-write defaults are explicit bundle fields.
6. Rules match bounded exact sets and one exact argument prefix. Policy v1 has no regular expressions or user code.
7. Policy bundles are canonicalized without their detached signature and identified by SHA-256.
8. Ed25519 is the only Policy v1 signature algorithm.
9. Signing keys are trusted or revoked through operator configuration outside workflow source.
10. A valid signature proves bundle provenance; it does not grant authority by itself.
11. The admitted base bundle is immutable for deterministic recovery.
12. A future emergency overlay may only deny; it cannot grant authority.
13. Missing required policy, a future contract, an unknown signing key, a revoked key, or an invalid signature fails closed.
14. Policies do not synthesize approval steps. They verify durable evidence produced by explicit workflow operations.
15. Static checks must report runtime-dependent outcomes as unknown rather than allowed.

## Capability Coverage Baseline

| Effect path | Current authority | Infrastructure descriptor required | IP4 posture |
| --- | --- | --- | --- |
| `services.process.run` | Process Policy v1 and Rust process handler | Executable alias, argument structure, cwd class, target, effect | Protected |
| `services.http.request` | Managed HTTP handler | Method, credential-free origin, path class, target | Protected |
| native `fetch` | Instrumented native-fetch authority | Method, credential-free origin, path class | Protected or blocked when no descriptor exists |
| `services.db` | Managed database handler | Driver, operation class, logical connection identity | Writes protected; reads follow bundle default |
| `services.storage` | Managed storage handler | Operation, logical backend, normalized key class | Writes and deletes protected |
| `services.cache` | Managed cache handler | Operation and logical key class | Mutations protected when policy marks the cache authoritative for coordination |
| `services.events.emit` | Durable event handler | Event name and destination scope | Protected |
| `services.workflows.call/start` | Durable workflow-call authority | Child workflow ID and call/start mode | Protected |
| `services.artifacts` | Artifact Policy v1 and artifact authority | Operation, immutable artifact identity, provenance | Writes, export, and materialization protected |
| `services.locks` | Durable lock authority | Key digest, ownership, fencing token | Acquire/release and evidence use protected |
| notification providers | Provider-specific handler | Provider, operation, destination class | Protected where messages create external effects |
| custom providers | Isolated provider host | Registered provider ID and reviewed adapter metadata | Denied under default-deny until an adapter exists |
| compensation and cleanup | Corrective runtime | Original saga/action identity and corrective mode | Protected by dedicated corrective rules |
| raw process APIs | Script-host isolation and module permissions | None | Must remain unavailable; policy is not a sandbox |
| direct uninstrumented network APIs | Script-host isolation and module permissions | None | Must remain unavailable or enforcement cannot claim coverage |

The table is the closed descriptor-coverage contract. IP2 covers the registered built-in capability paths shown above. Native `fetch` and custom providers remain unsupported and therefore fail closed until reviewed adapters exist.

## Operation Descriptor v1

Rust builds `woml.infrastructure-operation` v1 only after the generic capability envelope and the registered handler have validated the request. The descriptor binds:

- run, workflow, definition, node, attempt, invocation, and stable operation identity;
- registered capability, operation, effective effect class, environment, execution mode, and runtime-selected target;
- service-specific safe facts such as executable alias, HTTP origin and method, database operation class, workflow target, artifact identity, or provider identity;
- an optional engine-owned review digest.

Raw process arguments exist only in evaluator memory for exact prefix comparison. Serialized descriptors contain their count and a domain-separated digest. Database connections, storage/cache keys, lock sessions, and provider destinations are likewise represented by domain-separated digests. Authorization headers, credentials, request bodies, artifact bodies, provider tokens, environment values, and message text are excluded.

The policy metadata method is separate from presentation metadata and durable operation metadata. An adapter cannot silently fall back to arbitrary capability input: an unregistered capability/operation returns `WOML_POLICY_INPUT_UNSUPPORTED`.

## Evidence Reference v1

`woml.infrastructure-evidence` v1 is a Rust-owned typed reference, not a deserializable workflow payload. Its constructors are crate-private and reserved for durable WOML authorities. A JSON object that resembles an approval, artifact, attestation, lock, or operator record cannot become evaluator evidence.

Evidence binds its authority and digest to a run, with optional node and operation bindings. It also carries issue time, optional expiry, optional review-digest binding, and only the typed facts needed by its evidence kind. The evaluator rejects evidence that is stale, expired, cross-run, cross-step, cross-operation, integrity-unverified, or review-mismatched.

## Deterministic Evaluation

The immutable Rust evaluator sorts rules by ID and applies this precedence:

1. any matching deny rule denies;
2. every requirement on every matching allow rule must be satisfied;
3. a matching allow with satisfied requirements allows;
4. otherwise the bundle default for the effective operation class applies.

Decisions use `woml.infrastructure-decision` v1 and include the policy and descriptor digests, deterministic matched rule IDs, safe operation summary, reason codes, bounded requirement summaries, a bounded explanation, authority time, and an instruction count. Runtime duration is returned as telemetry but excluded from canonical decision serialization.

Prepared bundles are cached only by canonical policy digest. Decisions are never cached because evidence, time, target, and execution mode are context-dependent. Evaluation performs no I/O and has frozen limits of 250,000 instructions, 100 ms wall time, 128 evidence references, 1,024 rules, and 1,024 explanation bytes. Exhaustion denies with `evaluation_budget_exhausted`.

Focused debug-profile measurements evaluate the maximum 1,024-rule bundle within the 100 ms evaluator watchdog on the release development baseline. The compatibility fixture records the budget; enforcement in IP4 must not widen it implicitly.

## Policy Bundle Limits

- Maximum source size: 1 MiB
- Maximum JSON nesting: 16
- Maximum JSON values: 100,000
- Maximum rules: 1,024
- Maximum values in one matcher: 128
- Maximum approval roles: 64
- Maximum approval quorum: 20
- Maximum evidence age: 365 days
- Maximum lock minimum-remaining duration: 24 hours
- Duplicate object keys and unknown fields are rejected
- Numbers are bounded integers; Policy v1 has no floating-point values

## Canonical Identity

The canonical payload is the validated policy object with the root `signature` field removed, serialized with lexicographically ordered object keys and unchanged array order. Its identity is:

```text
sha256:<64 lowercase hexadecimal characters>
```

Reformatting or reordering object keys does not change the identity. Changing a value or array order does. This intentionally keeps author order visible in signed policy history even where later evaluation treats a set as unordered.

The detached Ed25519 signature covers the exact canonical payload bytes. Public keys are raw 32-byte Ed25519 keys encoded with canonical unpadded base64url. Signatures are 64 bytes in the same encoding.

## Rule and Requirement Boundary

Policy v1 can express requirements for:

- rich approval quorum, roles, freshness, and review-digest binding;
- immutable artifacts with verified integrity and optional review binding;
- exact lock ownership with a minimum remaining lease;
- an absolute UTC validity window.

The contract deliberately does not accept semantic labels such as `verifiedBackup: true` from workflow code. A later trusted attestation contract may add vendor-specific or domain-specific facts. Until then, unsupported facts fail validation or cannot satisfy a requirement.

## Recovery and Updates

The runtime will pin an admitted run to the exact base bundle digest. Reloading a same-named policy cannot silently change the authority of an existing run. Later phases must retain or durably reference the canonical admitted bytes.

Emergency revocation is separate: an operator-owned deny-only overlay may stop new effects in active runs. Because it cannot allow, it cannot widen the run's admitted authority.

## Failure Safety

A policy error must never be translated into an allow. Later enforcement maps failures to stable public codes:

- `WOML_POLICY_REQUIRED`
- `WOML_POLICY_INVALID`
- `WOML_POLICY_UNAVAILABLE`
- `WOML_POLICY_SIGNATURE_INVALID`
- `WOML_POLICY_DENIED`
- `WOML_POLICY_REQUIREMENTS_UNSATISFIED`
- `WOML_POLICY_INPUT_UNSUPPORTED`
- `WOML_POLICY_DECISION_UNAVAILABLE`

Diagnostics expose safe rule and requirement summaries only. They never include credentials, unrestricted payloads, complete command output, artifact bodies, or private reviewer assertions.

## Explicit IP0–IP3 Non-Goals

- Central runtime enforcement (planned for IP4)
- Automatic approval creation
- Vendor-specific Terraform, Kubernetes, or cloud-plan parsing
- Remote policy distribution
- Policy inheritance
- Rego, Cedar, JavaScript, or another embedded policy language
- A policy editor or hosted policy service
- Granting access denied by another WOML or operating-system authority

