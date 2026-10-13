# Durable Waits and External Signals v1

**Status:** reviewed release contract with two explicitly unsupported
composition variants listed below.

This document freezes the boundary between WOML authoring, Model v17, Event
v16, Store v15, external signal ingress, inspection, and recovery.

## Authoring contract

### Duration

```xml
<wait id="cooldown" name="Cooldown" duration="10m" />
```

`id` and a whole duration from 1 second through 365 days are required. `name` and `description` are
optional. The element is empty.

### Absolute instant

```xml
<wait-until id="window" at="{{context.steps.plan.opensAt}}" />
```

`at` is an RFC 3339 string literal or an exact visible context reference that
resolves to one. The element is empty.

### External signal

```xml
<wait-for
  id="result"
  signal="job.completed"
  correlation="{{context.steps.start.jobId}}"
  timeout="2h"
  on-timeout="fail"
>
  <schema>{ "type": "object" }</schema>
</wait-for>
```

The single-signal form uses `signal` and at most one direct `<schema>`. The
multi-signal form omits `signal` and contains 1 through 16 `<accept signal>`
children, each with at most one `<schema>`. Mixing the forms is invalid.

`correlation` is required and is either a bounded literal or an exact context
reference. `timeout` is optional. `on-timeout` is `fail` or `continue`, defaults
to `fail`, and is invalid without `timeout`.

Signal names are lowercase alphanumeric segments separated by `.`, `_`, or
`-`, contain at least one separator, and are at most 128 UTF-8 bytes.

## Durable identities

The logical wait identity is `(run_id, wait_id, iteration_identity)`. The v1
public runtime executes only the empty iteration identity. A signal request is
matched by `(run_id, wait_id, signal, correlation_digest)`.

An idempotency key is hashed before persistence. The same key and same request
return `duplicate`; the same key with different content returns
`identity_conflict`.

## Runtime state machine

```text
unregistered -> registered -> signaled | elapsed | timed_out | cancelled
```

Registration is committed before the scheduler releases execution resources.
Exactly one terminal transition may win. State is reconstructed by folding
Event v16; in-memory timers and promises are never authoritative.

## External admission v1

The internal admission envelope is `woml.external-signal-admission/v1`. The
public result is `woml.signal-result/v1` with:

- `status`: `accepted`, `duplicate`, `identity_conflict`,
  `already_resolved`, `target_not_found`, or `rejected`;
- stable `signalId`, `runId`, and `waitId`;
- optional `resolution`: `early` or `resolved`;
- optional stable error `code`.

The public HTTP route is:

```text
POST /v1/signals/<signal-name>
Authorization: Bearer <WOML_SIGNAL_TOKEN>
Content-Type: application/json
Idempotency-Key: <stable-logical-delivery-id>
```

Body:

```json
{
  "runId": "run_...",
  "waitId": "result",
  "correlation": "job-42",
  "payload": {}
}
```

Ingress is disabled when no signal token is configured. Authentication uses a
constant-time digest comparison. The endpoint accepts POST and JSON only.

## Bounds

| Boundary | Limit |
| --- | ---: |
| Canonical signal payload | 1,048,576 bytes |
| JSON nesting depth | 32 |
| Early signals retained per run | 64 |
| Signals accepted by one wait | 16 |
| Signal name | 128 bytes |
| Correlation and idempotency key | 256 bytes each |
| Public HTTP rate | 120 requests/minute/runtime |
| Due-wait scheduler batch | 1–1,000 records |

Oversized, malformed, unauthenticated, schema-invalid, correlation-mismatched,
or secret-bearing requests fail closed and cannot settle a wait.

## Result projection

Duration and absolute waits publish:

```json
{
  "status": "elapsed",
  "scheduledAt": "2026-10-13T09:00:01.000Z",
  "resumedAt": "2026-10-13T09:00:02.000Z"
}
```

Signals publish:

```json
{
  "status": "signaled",
  "signal": "job.completed",
  "correlation": "job-42",
  "payload": {},
  "receivedAt": "2026-10-13T09:00:02.000Z"
}
```

Timeout with `continue` publishes `status: "timed_out"`. Timeout with `fail`
selects the run failure and does not publish a successful wait result.

## Inspection and secrecy

Wait Inspection v1 and Run Inspection v8 expose bounded state, deadlines,
accepted signal names, resolved time, winning signal name, and a correlation
digest. They forbid raw correlations, payloads, idempotency keys, bearer
tokens, and resolved secret values.

## Recovery and maintenance

Store v15 indexes active targets and due deadlines. Startup discovers overdue
waits in `(deadline, run_id, wait_id)` order with bounded batches. Concurrent
signal, timeout, and cancellation transactions produce exactly one winner.
Verified backup/restore preserves pending wait rows and event history. Retention
does not select nonterminal waiting runs.

## Composition status

Main sequence, `<choose>`, `<fork>`, and `<parallel>` waits are executable and
recoverable. The runtime currently rejects or withholds publication for:

1. wait continuation identities inside individual `<for-each>` iterations;
2. a synchronous `services.workflows.call()` whose child run suspends.

These are release blockers for calling the complete feature generally
available; they are not silently approximated.
