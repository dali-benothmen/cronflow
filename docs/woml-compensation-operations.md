# Compensation and Cleanup Operations Runbook

Use this runbook when a run is compensating, cleaning up, or waiting for manual
correction.

## Inspect

```bash
woml get <runId> --state .woml/state.sqlite
woml get <runId> --state .woml/state.sqlite --json
```

Check the original outcome first. Then inspect the saga phase, unresolved
action ID, action kind, attempt count, ambiguity flag, and diagnostic code.
Inspection is bounded to 100 actions and 1 MiB; `truncated: true` means an
operator must use narrower internal tooling before acting.

## Decide safely

Choose retry only after proving the previous attempt did not perform the
external effect:

```bash
woml recover <runId> --retry <actionId> --state .woml/state.sqlite
```

If the effect was completed or independently repaired, record an external
resolution:

```bash
woml recover <runId> --resolve <actionId> \
  --note 'Verified under change ticket OPS-42' \
  --state .woml/state.sqlite
```

Recovery is single-writer and state checked. Repeating an already recorded
decision is rejected. Never resolve an action merely to make a run disappear
from an alert.

## Runtime behavior

An active runtime scans only correction work for workflows it owns and uses
the existing durable policy claim before execution. Scans are limited to 100
actions. If no runtime is active, start the same deployment against the same
state store; restart recovery reconstructs correction state and resumes only
known-safe work.

## Backup, restore, and retention

Backups preserve event history, correction indexes, and unresolved operator
decisions. Restore verifies integrity before replacement. Retention protects a
run while a saga is correcting, in manual recovery, or has a required recovery
record.

```bash
woml backup backups/pre-recovery --state .woml/state.sqlite
woml prune --before 30d --dry-run --state .woml/state.sqlite
```

Take a reviewed backup before database-level incident repair. Do not edit saga
or correction tables manually.

## Monitor

Correction Observation v1 exposes bounded counts:

- open sagas;
- pending corrective actions;
- active corrective attempts;
- actions requiring manual recovery; and
- age of the oldest manual-recovery action.

Alert on any manual-recovery action and on age exceeding the organization's
response objective. Correlate by run and safe action identity; metrics contain
no payloads or secrets.

## Escalation checklist

1. Preserve the run ID, action ID, original failure code, and external ticket.
2. Check whether the outside effect definitely happened.
3. Stop competing manual changes to the same resource.
4. Use retry only for proven non-execution; otherwise repair externally and
   resolve.
5. Confirm cleanup completed and the run reached its original terminal
   outcome.
6. Keep raw credentials and customer payloads out of notes and screenshots.
