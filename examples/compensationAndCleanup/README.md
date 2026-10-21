# Compensation and Cleanup

These workflows demonstrate explicit correction for operations that may partly
succeed across systems. WOML does not pretend those effects form one database
transaction: each completed effect declares how it can be compensated, and
each saga declares cleanup that must run before the scope settles.

| Example | Partial success being corrected |
| --- | --- |
| [`previewEnvironment.woml`](previewEnvironment.woml) | Network and database created before a preview deployment fails |
| [`deploymentRollback.woml`](deploymentRollback.woml) | A release is registered and deployed before verification fails |
| [`inventoryRelease.woml`](inventoryRelease.woml) | Stock reserved before order fulfillment fails |
| [`paymentRefund.woml`](paymentRefund.woml) | Payment captured before fulfillment fails |
| [`accessRevocation.woml`](accessRevocation.woml) | Temporary access granted before onboarding fails |
| [`temporaryResourceCleanup.woml`](temporaryResourceCleanup.woml) | Temporary workspace and credentials that must always be removed |

Validate every example:

```bash
woml check examples/compensationAndCleanup
```

Run the preview example once:

```bash
woml test examples/compensationAndCleanup/previewEnvironment.woml
```

The examples use deterministic local JavaScript so they are safe to run. In a
real workflow, replace the simulated operations with managed services or local
modules and preserve the returned external identifiers. A compensation must
target exactly the resource created by its forward step.

Read [Compensation and Cleanup](../../docs/woml-compensation-and-cleanup.md)
before adapting these patterns. Compensation is corrective work, not a claim
that the outside world was restored exactly.
