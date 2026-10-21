# Rich Plan Approvals

Rich Plan Approvals answer a stricter question than “may this workflow
continue?” They let a reviewer authorize one immutable proposal: this plan,
these artifacts, this workflow definition, this quorum, and these required
roles.

Use a basic `<approval>` for an ordinary yes/no checkpoint. Use a rich approval
when the exact evidence matters—for example a Terraform plan, database change,
AI-proposed remediation, access grant, or destructive infrastructure action.

## The authoring shape

Build a Review Payload v1 object in an earlier step, then reference that exact
durable output:

```xml
<step id="buildReview">
  <script>
    return {
      review: {
        title: "Deploy payments-api to production",
        summary: "Apply the verified release plan.",
        risk: "high",
        facts: [{ label: "Environment", value: "production" }],
        changes: [{ action: "update", target: "deployment/payments-api", summary: "Deploy release 42" }],
        checks: [{ name: "Integration tests", status: "passed" }],
        artifacts: []
      }
    };
  </script>
</step>

<approval
  id="approveDeployment"
  review="{{context.steps.buildReview.review}}"
  approvals="2"
  required-roles="production-owner security-owner"
  timeout="24h"
  on-timeout="reject"
>
  <when-approved>...</when-approved>
  <when-rejected>...</when-rejected>
</approval>
```

The `review` attribute accepts only an exact reference to already-visible
durable step context. It cannot read `secrets`, call a service, use a dynamic
template, or reference a future/sibling result. `approvals` is 1–20.
`required-roles` contains at most 16 lowercase role names.

## Review Payload v1

| Field | Purpose |
| --- | --- |
| `title` | Short statement of the decision. |
| `summary` | Bounded explanation of the proposed effect. |
| `risk` | `low`, `medium`, `high`, or `critical`. |
| `facts` | Labels and values that help reviewers understand context. |
| `changes` | `create`, `update`, `replace`, `destroy`, or `no-op` effects. |
| `checks` | Passed, failed, warning, skipped, or unknown validation evidence. |
| `artifacts` | Durable Artifact references whose exact digests are authorized. |

WOML rejects unknown fields, oversized/deep values, malformed Unicode, known
secret values, forged artifacts, and artifact bodies. Notification providers
receive a bounded presentation—not raw files, tokens, credentials, or arbitrary
HTML.

## Configure the Reviewer Directory

Reviewer roles come from operator configuration, never from workflow source or
provider callback data. Create a JSON file:

```json
{
  "profile": "woml.reviewer-directory/v1",
  "version": "production-reviewers-2026-10-21",
  "principals": [
    {
      "id": "person:alice",
      "roles": ["production-owner"],
      "identities": [
        { "provider": "slack", "accountId": "T0123456789", "actorId": "U0123456789" }
      ]
    }
  ]
}
```

Then activate the workflow with:

```bash
woml run workflow.woml --reviewer-directory ./reviewer-directory.json
```

WOML validates the closed schema, version, unique principal IDs, unique provider
identities, sorted roles, provider names, and bounded values before activation.
Missing or insufficient reviewer authority fails with
`WOML_APPROVAL_AUTHORITY_UNAVAILABLE` before any protected effect runs.

### Provider identity values

- **Slack:** `accountId` is the workspace/team ID (`T…`); `actorId` is the
  reviewer’s user ID (`U…`). In Slack, open the workspace/user profile and use
  **Copy member ID**; obtain the workspace ID from the app authentication
  response or workspace settings.
- **Telegram:** `accountId` is the numeric bot ID returned by `getMe`; `actorId`
  is the reviewer’s numeric Telegram user ID from the verified callback.
- **Discord:** `accountId` is the bot application user ID; `actorId` is the
  reviewer’s user snowflake. Enable Developer Mode and use **Copy User ID**.
- **trusted-http:** use only behind an identity gateway that authenticates the
  request and supplies a verified account/actor assertion. Never accept IDs
  directly from a browser form or request body as proof of identity.

The same person may have several provider identities, but their canonical
principal counts once. One principal may represent several required roles and
still contributes only one approval.

## Quorum and rejection

An approval resolves only after both conditions are true:

1. the number of distinct approving principals reaches `approvals`; and
2. every `required-roles` role is represented by those principals.

The first eligible rejection resolves the review as rejected. Duplicate votes
from the same principal are idempotent; a conflicting repeat is denied. Votes,
timeout, cancellation, artifact invalidation, and final quorum race in one
Rust-owned transaction, so only one final result wins.

## Review and inspect a run

While waiting, the terminal and local approval page show title, risk, summary,
key changes, checks, artifacts, digest prefix, deadline, and quorum progress.
Provider messages use the same bounded presentation. The complete operator view
is available without opening SQLite:

```bash
woml get run_<id>
woml get run_<id> --json
woml run run_<id> --logs
```

Run Inspection v11 includes review metadata, artifact references, required and
received approvals, represented roles, deadline, Reviewer Directory version,
and a bounded reviewer decision history. Operator output may contain canonical
principal and provider-account identifiers; ordinary script context never does.

## Restart, recovery, backup, and retention

Partial quorum is durable. Stop WOML after the first vote and restart the same
workflow/state database: the immutable review, first vote, role coverage,
artifact pins, and remaining quorum are reconstructed without replaying the
proposal step.

Use the normal production controls:

```bash
woml backup ./backup --state .woml/workflow-history.sqlite
woml restore ./backup --state .woml/workflow-history.sqlite
woml prune --state .woml/workflow-history.sqlite
```

Backup preserves pending reviews, decisions, reviewer-directory version, and
artifact pins together. Restore validates and migrates the temporary copy before
replacement. Pruning cannot remove evidence or artifacts still protected by an
active review; settled records follow configured retention.

## Security and privacy model

- Rust is the sole authority for review validation, immutable binding, identity
  mapping, quorum, and final settlement.
- The digest binds workflow/definition identity, run/request identity, canonical
  review content, artifact metadata/digests, quorum policy, roles, and directory
  version.
- Provider callbacks cannot submit or replace review content or roles.
- Artifact bytes are never embedded in messages or run events.
- Tokens, credentials, secret values, raw identity assertions, and provider
  actor IDs are excluded from normal workflow output and notification text.
- Provider failure is isolated: another successful destination remains usable,
  and one final resolution is shared across every delivery.

See [Rich Plan Approvals v1 protocol](protocols/rich-plan-approvals-v1.md) for
frozen limits and wire contracts, and
[the examples](../examples/richApprovals/README.md) for Terraform and
AI-remediation workflows.
