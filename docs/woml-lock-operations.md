# Locks and Leases Operations Runbook

## Inspect contention

Use the normal run inspection:

```bash
woml get <run-id>
```

Look for `waiting`, `held`, `released`, `timed_out`, `cancelled`, or `lost`.
Compare the acquisition deadline, current expiry, wait duration, hold duration,
and fencing token. A long queue usually means a slow holder, a key that is too
broad, or an operation holding ownership across an unnecessary wait.

## Respond to common failures

| Code | Meaning | Operator action |
| --- | --- | --- |
| `WOML_LOCK_ACQUIRE_TIMED_OUT` | The run did not acquire before its deadline. | Inspect the holder; increase the timeout only if the wait is expected. |
| `WOML_LOCK_WAIT_LIMIT` | A key or store reached its waiting ceiling. | Stop duplicate producers, inspect the hot key, then retry after backlog falls. |
| `WOML_LOCK_LEASE_LOST` | Renewal or current ownership could not be proven. | Treat work as stale; verify the target using the fencing token before retrying. |
| `WOML_LOCK_FENCING_EXHAUSTED` | The signed token space is exhausted. | Stop the key and perform reviewed operator recovery; never reset or reuse tokens. |
| `WOML_LOCK_AUTHORITY_UNAVAILABLE` | SQLite or the runtime authority is unavailable. | Restore store health before retrying protected effects. |

## Backup, restore, retention, and prune

Backups include lock resources and waiters. Restore deliberately clears live
owners and marks their held waiter records lost while preserving fencing
high-water. Retention protects runs that own or wait for a live lock. Once a
terminal waiter is pruned, its resource row remains so future grants cannot
reuse an older token.

Before activating a restored database:

1. stop or permanently isolate the old runtime;
2. verify the backup and definition inventory;
3. restore through the WOML command, not a raw file copy into a live runtime;
4. inspect lost lock scopes and protected external systems;
5. activate one runtime authority;
6. retry only operations whose target state and fencing evidence are known.

## Capacity and performance

The reviewed local profile permits 1,024 waiters per key and 10,000 per store.
Recovery scans no more than 250 due items per pass. Performance budgets live in
`examples/locksAndLeases/performance-budgets.v1.json` and measure Rust store
operations after startup; they never justify weakening durability or fencing.

Choose specific keys such as `production/payments-api`, not a global
`production` key, unless every protected operation truly must serialize.
