# Locks and Leases

Locks and leases let WOML serialize work that must not overlap across runs or
local runtime processes. Rust stores the wait queue, owns lease renewal, and
issues a strictly increasing fencing token whenever ownership changes. A
waiting run releases its worker and execution slot; it does not poll in
JavaScript.

Use this feature for deployments, database migrations, restore cutovers,
credential rotation, cache publication, and any shared resource where two
otherwise-valid workflow runs would be unsafe together.

## Start with a durable lock scope

```xml
<lock
  id="deployment"
  name="Production deployment"
  key="production/{{context.steps.plan.service}}"
  acquire-timeout="15m"
  lease="45s"
>
  <step id="deploy" name="Deploy service">
    <script>
      return {
        service: context.steps.plan.service,
        fencingToken: context.lock.fencingToken
      };
    </script>
  </step>

  <step id="verify" name="Verify rollout">
    <script>return { healthy: true };</script>
  </step>
</lock>
```

The key is the coordination boundary. Two runs using the same key are ordered;
different keys can proceed independently. Keys are 1–256 characters, cannot
contain whitespace padding, control characters, or empty `/` segments, and
must never contain secrets or raw payloads.

`acquire-timeout` defaults to the reviewed contract value and may range from
one second through seven days. `lease` may range from five seconds through ten
minutes. Rust renews a live lease; workflow JavaScript does not implement a
heartbeat.

## Understand the result and context

Inside the scope, `context.lock` is available:

```js
context.lock.id
context.lock.key
context.lock.fencingToken
context.lock.acquiredAt
context.lock.expiresAt
```

After release, the lock result is available at `context.steps.<lockId>`. It is
bounded JSON and includes the scope's declared result. Internal lease IDs,
renewal credentials, runtime ownership, and input digests are never exposed.

## Use the JavaScript convenience API for a short critical section

When only part of one script needs serialization, use the attempt-scoped
facade:

```js
return await services.locks.withLease(
  `inventory/${context.payload.customerId}`,
  {
    name: "reserve-customer-inventory",
    acquireTimeout: "10s",
    lease: "15s"
  },
  async lease => {
    await lease.assertCurrent();
    const result = await services.http.request({
      method: "POST",
      url: "https://inventory.example/reservations",
      body: {
        customerId: context.payload.customerId,
        fencingToken: lease.fencingToken
      }
    }, { name: "reserve-inventory" });
    return { accepted: result.ok, fencingToken: lease.fencingToken };
  }
);
```

The callback API is not a durable closure. If its process crashes, WOML does
not recreate arbitrary JavaScript stack state. Use markup `<lock>` when the
protected operation includes several steps, durable waits, approval, restart
recovery, or other orchestration boundaries.

## What happens under contention

For one key, Store v16 keeps one owner and a FIFO waiter queue. Registration is
idempotent: the same execution identity and input reattach; reuse with changed
input fails. When the owner releases or expires, the oldest eligible waiter is
granted the next fencing token atomically.

Safety ceilings prevent a hot key from exhausting the store:

- 1,024 waiting acquisitions per key;
- 10,000 waiting acquisitions per local store;
- recovery reads at most 250 due items per batch.

An uncontended key may still start when unrelated keys have filled the waiting
backlog. That keeps one noisy resource from stopping independent work.

## Cancellation, timeout, failure, and restart

- An acquisition timeout settles the waiter and never lets it acquire later.
- Cancelling a waiter removes it from eligibility.
- A proven owner releases on success or failure.
- An expired owner becomes stale and cannot report successful protected work.
- A runtime restart reattaches to durable ownership when it is still current.
- Restore begins a new runtime epoch: live owners are cleared, their waiters
  become `lost`, and fencing high-water remains unchanged.

Inspection is available through the normal operator command:

```bash
woml get <run-id>
```

It shows status, key, wait duration, hold duration, deadline, expiry, and safe
failure codes without credentials or workflow payloads.

## Run the examples

```bash
woml check examples/locksAndLeases
woml run examples/locksAndLeases/deployment.woml
```

The infrastructure examples use approved process aliases. Configure Process
Policy v1 before executing their commands. See
[Secure process execution](woml-process.md), the
[fencing guide](woml-lock-fencing.md), and the
[operations runbook](woml-lock-operations.md).
