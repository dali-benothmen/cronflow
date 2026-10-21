# Rich Plan Approval examples

These workflows turn a proposed production effect into immutable, bounded
review evidence. Rust binds the review, artifact digests, workflow definition,
quorum, required roles, and Reviewer Directory version into one SHA-256 digest.

- `terraformDeployment.woml` preserves a Terraform plan as a durable artifact,
  requires two roles, and applies the exact materialized artifact after quorum.
- `aiRemediation.woml` demonstrates the safe shape for an AI-generated
  infrastructure recommendation: diagnosis, review, trusted decision, effect.
- `reviewer-directory.example.json` maps provider identities to canonical
  people and operator-owned roles. Replace every sample account and actor ID.

Validate without executing infrastructure tools:

```bash
woml check examples/richApprovals/
```

Run with explicit operator policies and reviewer authority:

```bash
woml run examples/richApprovals/terraformDeployment.woml \
  --process-policy ./process-policy.json \
  --artifact-policy ./artifact-policy.json \
  --reviewer-directory ./reviewer-directory.json
```

Never use the example identities unchanged in production.
