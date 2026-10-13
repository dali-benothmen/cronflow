# Durable Waits and External Signals

These workflows show how a WOML run can stop consuming execution resources,
survive a runtime restart, and continue later from a durable timer or an
authenticated external callback.

| Example                                            | What it demonstrates                                  |
| -------------------------------------------------- | ----------------------------------------------------- |
| [`timer.woml`](timer.woml)                         | A fixed-duration durable pause.                       |
| [`maintenanceWindow.woml`](maintenanceWindow.woml) | Waiting until a calculated RFC 3339 instant.          |
| [`ciCallback.woml`](ciCallback.woml)               | Two accepted CI result signals with one winner.       |
| [`securityScan.woml`](securityScan.woml)           | Schema-validated scan results followed by a decision. |

Start with the timer:

```bash
woml run examples/durableWaits/timer.woml
```

For callback examples, configure the dedicated ingress credential first:

```bash
woml secrets set WOML_SIGNAL_TOKEN
woml run examples/durableWaits/ciCallback.woml
```

The runtime prints the authenticated HTTP route. Local operators can also use
`woml signal`; applications and CI systems should call the HTTP route directly.
See the [Durable Waits guide](../../docs/woml-durable-waits.md) for the complete
walkthrough and security model.
