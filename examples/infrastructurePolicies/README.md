# Infrastructure Policy examples

This directory demonstrates the complete Policy v1 production boundary rather
than treating a policy file as a replacement for normal capability security.

`reviewedTerraformRelease.woml` creates a Terraform plan, stores the plan and
an independently prepared recovery plan as durable artifacts, binds both to a
two-person rich approval, acquires the production deployment lease, applies the
reviewed bytes, verifies the result, and uses the reviewed recovery artifact as
the saga compensation if verification fails.

Before using it, replace every sample identity and executable path. Generate
`.woml/artifact-input/recovery.tfplan` through your reviewed recovery process;
the example deliberately does not invent a Terraform rollback plan.

Validate the workflow and policy without executing Terraform:

```bash
woml policy check examples/infrastructurePolicies/production-policy.example.json
woml policy explain examples/infrastructurePolicies/reviewedTerraformRelease.woml \
  --policy examples/infrastructurePolicies/production-policy.example.json
woml check examples/infrastructurePolicies/reviewedTerraformRelease.woml \
  --policy examples/infrastructurePolicies/production-policy.example.json
```

After configuring real policies, reviewer identities, and plan files:

```bash
woml run examples/infrastructurePolicies/reviewedTerraformRelease.woml \
  --policy examples/infrastructurePolicies/production-policy.example.json \
  --process-policy examples/infrastructurePolicies/process-policy.example.json \
  --artifact-policy examples/infrastructurePolicies/artifact-policy.example.json \
  --reviewer-directory examples/infrastructurePolicies/reviewer-directory.example.json
```

During an incident, the operator may restart the runtime with the separately
reviewed `emergency-deny.example.json`. An emergency overlay only removes
authority; it never changes the run-pinned base policy or grants a new action.

Static `check` and `policy explain` output is advisory. Only the Rust authority
can see the final normalized operation and current durable evidence, and only
its pre-effect decision authorizes dispatch.
