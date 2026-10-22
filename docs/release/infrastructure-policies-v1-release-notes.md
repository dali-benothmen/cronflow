# Infrastructure Policies v1

Infrastructure Policies add an operator-owned, deny-capable authorization layer
immediately before protected WOML effects. Workflow source describes an action;
the Rust authority decides whether that normalized action is allowed in the
selected environment and whether current durable evidence satisfies its rules.

## Released surface

- Strict JSON Policy Bundle v1 with canonical SHA-256 identity and optional
  Ed25519 verification against operator-owned trust.
- Trusted adapters for the built-in protected capability inventory.
- Immutable per-run policy binding and bounded durable decision history.
- Engine-owned approval, artifact-integrity, and current-lock evidence.
- Deny-only emergency overlays that cannot grant authority.
- `woml policy check`, `woml policy explain`, policy-aware `woml check`, and
  runtime `--policy` activation.
- Safe operator inspection without raw arguments, credentials, artifact bytes,
  reviewer assertions, or workflow payloads.

Existing workflows retain their documented capability behavior when no
Infrastructure Policy is activated. Supplying policy trust or requiring a
signature without selecting a policy fails closed.

## Security boundary

Policy can remove authority or require durable evidence. It cannot grant access
denied by Process Policy, Artifact Policy, provider credentials, filesystem or
network isolation, target restrictions, cancellation, or the operating system.
Static explanation is advisory; runtime authorization always belongs to Rust.

## Deferred, not implied

Policy v1 does not parse Terraform plans, Kubernetes manifests, IAM changes, or
SQL semantics. It does not provide remote attestation, organization-wide policy
distribution, inheritance, or a second embedded policy language. Those require
new reviewed authorities and versioned contracts.
