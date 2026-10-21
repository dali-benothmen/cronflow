# WOML Rich Plan Approvals v1

**Status:** Frozen by RPA0  
**Authoring:** `<approval review="…" approvals="…" required-roles="…">`  
**Versions:** Model v20 · Event v19 · Store v18 · Approval HTTP v2 · Notification Provider Host v3 · Approval Inspection v2

## Purpose

Rich Plan Approvals bind a human decision to one immutable, bounded proposal. They extend the existing Human Approval feature; they do not replace or reinterpret basic approvals.

## Authoring contract

`review` is an exact `{{context.steps.<priorId>...}}` reference. It cannot use trigger payloads, secrets, services, literals, iteration bindings, templates, dynamic expressions, sibling fork output, or a forward reference. Its runtime value must be an object matching Review Payload v1.

`approvals` is a decimal integer from 1 through 20 and defaults to `1`. `required-roles` is a space-separated list of at most 16 unique role names. Roles are lowercase, no longer than 64 characters, and match `^[a-z][a-z0-9]*(?:[-_.][a-z0-9]+)*$`. The compiler sorts roles before hashing and lowering. Both policy attributes are invalid without `review`.

A basic `<approval>` without these attributes compiles exactly as before. A workflow containing a rich approval compiles as Model v20. Model v20 retains the complete Model v19 graph and adds three fields to the owning `engine.approval-wait` node: `review`, `requiredApprovals`, and `requiredRoles`. Either all three fields are present or none are.

RPA1 freezes and validates the reference shape statically. Because WOML step outputs are ordinary JavaScript values, the Rust authority validates that the resolved value is an object when execution reaches the approval.

## Review Payload v1

The authoritative schema is `review-payload.v1.schema.json`. The serialized RFC 8785 JSON is limited to 65,536 UTF-8 bytes and 8 levels of nesting. Strings must be valid Unicode without control characters other than normal JSON whitespace.

| Field | Limit |
|---|---:|
| `title` | required, 1–160 characters |
| `summary` | required, 1–2,000 characters |
| `risk` | `low`, `medium`, `high`, or `critical` |
| `facts` | 32 rows; label 64, value 512 characters |
| `changes` | 100 entries; target 256, summary 1,000 characters |
| `checks` | 64 entries; name 128, explanation 1,000 characters |
| `artifacts` | 32 Durable Artifact Reference v1 values |

Unknown fields fail closed. Artifact bodies, arbitrary HTML, remote images, executable Markdown, callback-supplied review content, paths, credentials, and secret values are forbidden.

## Immutable binding

Rust evaluates and validates the already-durable review context, resolves and verifies every artifact, scans known secret values, canonicalizes the binding with RFC 8785, and hashes it with SHA-256. The ordered binding inputs are:

1. definition hash;
2. workflow ID and workflow version;
3. run ID;
4. approval ID;
5. request ID;
6. canonical Review Payload v1 snapshot;
7. ordered artifact IDs, sizes, media types, and SHA-256 digests;
8. required approval count and sorted required roles;
9. Reviewer Directory version.

Any meaningful change creates a different `sha256:<64 lowercase hex>` review digest. Decision capabilities bind to the request, delivery, and review digest. Provider callbacks cannot replace any reviewed field.

## Reviewer Directory v1

The directory is operator-owned configuration conforming to `reviewer-directory.v1.schema.json`. An authenticated `(provider, accountId, actorId)` identity maps to one canonical principal and its roles. WOML source and callback JSON cannot grant identity or roles.

Directory validation rejects duplicate principal IDs, duplicate provider identities, unsorted or duplicate roles, unknown providers, and unbounded values. Activation fails closed when a rich approval requests multiple people or roles but no trusted reviewer authority exists. The directory version used for a pending request is durable and remains stable for that request.

## Quorum and races

- Quorum counts distinct canonical principals, never clicks, tokens, deliveries, channels, or provider accounts.
- The same person mapped through several providers counts once.
- One principal may cover several required roles but still contributes one person.
- Approval resolves only after both the distinct-principal count and every required role are satisfied.
- The first eligible rejection resolves the request as rejected.
- Repeating the same decision by the same principal is idempotent; a conflicting repeat is rejected and audited.
- Unknown or ineligible identities never affect quorum.
- Token-only Approval HTTP v1 remains valid for basic approvals only.
- Approval, rejection, timeout, cancellation, and artifact invalidation compete in one atomic Rust transaction. Exactly one terminal result wins.

## Versioned boundaries

Model v20 describes the reference and authority policy. Event v19 records the immutable request, individual canonical-principal decisions, derived quorum progress, invalidation, and final resolution. Store v18 adds append-only review, decision, and artifact-pin indexes while events remain authoritative. Approval HTTP v2 requires the review digest and a trusted identity-gateway assertion. Provider Host v3 receives only a bounded presentation and a digest-bound capability. Inspection v2 returns bounded review and decision history; canonical and provider identities are protected operator data and never enter normal script context.

RPA0 freezes these JSON contracts. RPA1 implements Model v20 authoring and structural validation. Runtime validation, persistence, ingress, and execution are implemented in RPA2 through RPA5; until then, Model v20 execution must fail closed rather than silently use basic approval semantics.

## Public output

After final settlement, the selected arm receives a compact output containing the decision, source, decision time, review digest, title, risk, authorized artifact references, required count, received count, and required roles. Principal IDs, provider actor IDs, directory entries, tokens, credentials, and delivery internals are excluded.

## Stable diagnostics

The contract reserves `WOML_APPROVAL_REVIEW_INVALID`, `WOML_APPROVAL_REVIEW_TOO_LARGE`, `WOML_APPROVAL_REVIEW_SECRET_DETECTED`, `WOML_APPROVAL_ARTIFACT_INVALID`, `WOML_APPROVAL_BINDING_MISMATCH`, `WOML_APPROVAL_REVIEWER_UNKNOWN`, `WOML_APPROVAL_REVIEWER_FORBIDDEN`, `WOML_APPROVAL_DUPLICATE_REVIEWER`, `WOML_APPROVAL_AUTHORITY_UNAVAILABLE`, `WOML_APPROVAL_ALREADY_RESOLVED`, and `WOML_APPROVAL_EXPIRED` for runtime authority failures. Compiler diagnostics use the source-located `WOML_APPROVAL_REVIEW_*`, `WOML_APPROVAL_COUNT_INVALID`, and `WOML_APPROVAL_ROLE_*` families.

Diagnostics may describe corrective action but must never include token values, secret values, raw assertions, artifact bodies, or unnecessary personal data.
