# Locks and Leases v1

**Status:** frozen by LL0; authoring and Model v18 lowering are implemented by
LL1. Durable execution remains unavailable until the Rust authority is built.

This contract binds WOML authoring, Compiled Model v18, Run Event v17, Store
v16, Lock Inspection v1, and the future `services.locks.withLease()` capability.

## Authoring

```xml
<lock
  id="deployment"
  name="Production deployment"
  key="production/{{context.payload.service}}"
  acquire-timeout="10m"
  lease="30s"
>
  <step id="deploy">
    <script>
      return {
        fencingToken: context.lock.fencingToken
      };
    </script>
  </step>
</lock>
```

`id`, `key`, and `acquire-timeout` are required. `name`, `description`, and
`lease` are optional. The default lease is 30 seconds. The body contains one or
more ordinary flow items.

Keys are case-sensitive operator-visible UTF-8 resource identities. The
resolved key contains 1–256 bytes and has no surrounding whitespace, control
characters, or empty slash segments. Interpolation may use context visible at
the lock's DAG position. Secret interpolation is always rejected.

Acquisition timeout is 1 second through 7 days. Lease duration is 5 seconds
through 10 minutes. Nested and reentrant acquisition are unsupported in v1.

## Scope and result

Inside the body, scripts receive read-only `context.lock.id`, `.key`,
`.fencingToken`, and `.acquiredAt`. The token is an unsigned decimal string.

Body step outputs are visible only inside the lock. After durable release, only
the public lock ID becomes visible:

```json
{
  "status": "released",
  "key": "production/payments",
  "fencingToken": "42",
  "acquiredAt": "2026-10-20T10:00:00.000Z",
  "releasedAt": "2026-10-20T10:03:12.000Z",
  "waitedMs": 418
}
```

Timeout, cancellation, and lease loss do not publish this successful result.

## Ownership state machine

```text
unregistered -> waiting -> held -> released
                       \-> timed_out
                       \-> cancelled
                                  held -> lost
```

Registration is idempotent for the exact run, node, branch/iteration execution,
and acquisition generation. A conflicting key or timing policy is rejected.
One Store v16 transaction grants the oldest eligible waiter, advances the
per-key fencing high-water mark, commits ownership, and records acquisition.

Rust renews a held lease at one third of its duration. Routine renewal updates
compact coordination state and does not append immutable run events. Waiting
and renewal retain no Bun worker.

## Fencing guarantee

WOML checks current ownership before admitting a managed capability effect.
The protected target must also reject fencing tokens older than the greatest
token it has accepted for the strongest stale-writer guarantee.

Raw `fetch()`, arbitrary modules, external commands, and systems that ignore
the token cannot be automatically fenced. An already-issued remote side effect
may therefore be ambiguous after lease loss. WOML reports that ambiguity and
does not claim rollback.

## Event v17

The immutable vocabulary is:

- `lock_wait_registered`
- `lock_acquired`
- `lock_released`
- `lock_acquire_timed_out`
- `lock_acquisition_cancelled`
- `lock_lease_lost`

Exactly one terminal acquisition outcome may win. A released lease cannot be
lost later, and a stale owner cannot renew or release a newer lease.

## Store v16 and recovery

Store v16 separates immutable run history from compact `lock_resources` and
`lock_waiters` coordination records. It guarantees one live owner per key,
monotonic fencing, unique acquisition identity, FIFO eligibility, and atomic
expiry takeover.

Recovery reattaches waiting acquisitions. Committed ownership resumes only
when the protected continuation is safe. Interrupted effects follow WOML's
fail-closed no-replay rule. An expired owner is marked lost before any
continuation executes.

Backups preserve fencing high-water marks and clear live ownership. The old
authority must be retired before a restored authority starts; running both is
unsupported split brain.

## Cancellation and failures

Cancelling a waiter prevents later acquisition. Cancelling an owner releases
early only after all WOML-supervised protected work is proven stopped;
otherwise expiry decides ownership.

The stable error and retryability matrix is frozen in
`woml/tests/fixtures/locks-and-leases/errors.v1.json`. Lease loss and stale
owner failures are ambiguous because an external effect may already have been
issued.

## Programmatic API boundary

`services.locks.withLease()` is an attempt-scoped convenience. Rust owns its
lease session and hidden acquire/assert/release frames. The JavaScript callback
is not durable or reconstructed after a crash. Multi-step, approval, waiting,
and crash-resumable coordination use `<lock>`.

## Bounds

- 1,024 waiters per key;
- 10,000 waiting acquisitions per local store;
- one held lock per execution scope;
- bounded inspection pages of 100 records; and
- no public force-unlock operation.

SQLite coordinates processes sharing one state file on one machine. It does
not provide a multi-host distributed lock. A future shared backend requires a
separate reviewed conformance profile.
