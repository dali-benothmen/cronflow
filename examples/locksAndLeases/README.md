# Locks and Leases

These examples serialize operations that must not overlap, even when several
workflow runs or WOML processes use the same durable store.

| Example | Protected operation |
| --- | --- |
| [`deployment.woml`](deployment.woml) | One production deployment per service |
| [`databaseMigration.woml`](databaseMigration.woml) | One schema migration per database |
| [`backupRestore.woml`](backupRestore.woml) | Backup verification and restore cutover |
| [`credentialRotation.woml`](credentialRotation.woml) | One credential rotation per account |
| [`cacheRebuild.woml`](cacheRebuild.woml) | One cache rebuild per namespace |

Start with the deployment example:

```bash
woml run examples/locksAndLeases/deployment.woml
```

Open a second terminal against the same project and start it again. One run
owns `production/payments-api`; the other waits durably without occupying a Bun
worker. When ownership transfers, the next run receives a greater fencing
token.

The examples use `services.process.run()` for infrastructure commands. Review
the aliases and paths, then provide an approved Process Policy v1 file:

```bash
woml run examples/locksAndLeases/deployment.woml \
  --process-policy examples/process/process-policy.example.json
```

Read [Locks and Leases](../../docs/woml-locks-and-leases.md) before adapting
these examples to production. A lock coordinates ownership; the protected
system must honor the fencing token when stale-owner protection matters.
