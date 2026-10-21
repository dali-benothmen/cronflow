# Compensation and Cleanup v1

**Status:** frozen by CC0; authoring and Model v19 lowering are implemented by CC1. Durable execution begins in CC2.

This contract binds WOML authoring, Compiled Model v19, Run Event v18, Durable Store v17, Script Host Protocol v11, Correction Inspection v1, and operator recovery.

## Authoring

- `<saga id="...">` owns one non-empty forward flow and an optional final `<cleanup>` block.
- A forward `<step>` inside a saga may contain one trailing `<compensate>` with exactly one `<script>`.
- `<compensate>` accepts optional `timeout` and `retry` attributes.
- `<cleanup>` contains one or more sequential `<step>` elements; cleanup step `timeout` and retry attributes use the existing WOML grammars.
- Nested sagas, detached fork branches, corrective waits, corrective approvals, and multi-step compensations are rejected in v1.

## Fixed execution semantics

- Forward failure or cancellation stops new saga work and corrects only proven, successfully completed eligible actions.
- Registration and forward success are one durable transition.
- Compensation order is reverse durable completion sequence, never inferred source order.
- Cleanup always runs after compensation, or immediately after successful forward work, in source order.
- Every eligible corrective action is attempted even when another corrective action fails.
- The original failure or cancellation remains the primary run outcome.
- A committed saga is not reopened by a later failure outside its boundary.

## Exactly-once honesty

Corrective attempts use stable identities derived from the run, saga, forward execution, corrective action, and attempt. A disappearance after a side effect may have occurred is recorded as ambiguous and enters manual recovery; it is never replayed automatically.

## Context

Compensation receives a bounded read-only `context.compensation` binding with `sagaId`, `forStepId`, and `reason`. Cleanup receives `context.cleanup` with `sagaId`, `reason`, and `compensationStatus`. These values are ephemeral Script Host inputs and never mutable authoritative workflow context.

## Recovery

The frozen command direction is `woml recover <runId> --retry <actionId>` or `woml recover <runId> --resolve <actionId> --note <text>`. External resolution records `externally_resolved`; it never fabricates a successful execution event. The command is implemented in CC6.

## Limits

The reviewed v1 limits are 64 sagas per workflow, 10,000 compensation registrations per run, 64 cleanup steps per saga, 10 attempts per corrective action, 10,000 unresolved recovery rows per store, 100 inspection rows, a 1 MiB inspection response, and 100 recovery actions per scheduler batch.
