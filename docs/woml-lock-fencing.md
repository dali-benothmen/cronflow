# Lock Fencing Guide

A lease answers “who currently owns this key?” A fencing token answers the
harder question: “how can the protected system reject an old owner that wakes
up after ownership moved?”

Each grant for one key receives a token greater than every earlier token. WOML
persists that high-water value through release, retention, backup, restore,
and prune. It never resets the counter when a run disappears.

## The stale-owner problem

Suppose run A receives token 41 and pauses long enough for its lease to expire.
Run B then receives token 42. If A resumes and sends an ordinary write, a lock
alone cannot force the remote database or deployment controller to reject it.

The target must remember the greatest token it has accepted and reject smaller
tokens:

```sql
UPDATE deployment_targets
SET desired_version = ?, last_fencing_token = ?
WHERE service = ? AND last_fencing_token < ?;
```

Treat zero updated rows as stale ownership. The same pattern works with a
conditional object-store write, compare-and-set record, Kubernetes annotation,
or API idempotency/fencing header.

## Pass tokens only where they are meaningful

Inside markup locks use `context.lock.fencingToken`; inside
`services.locks.withLease()` use `lease.fencingToken`. Send the token to the
managed effect or target that enforces ordering. Do not log it as a secret—it
is not a credential—but do keep it with audit metadata so operators can explain
which owner won.

Managed WOML effects assert current ownership before dispatch where supported.
That closes the common in-process stale-owner race. It cannot make an arbitrary
external API honor fencing, so strong cross-system protection still belongs at
the target.

## Restore rule

A restored store preserves every resource's fencing high-water and clears live
ownership. The old authority must be retired before the restored runtime is
activated. Running both creates split brain; no local SQLite mechanism can
coordinate two independent copies after the restore boundary.
