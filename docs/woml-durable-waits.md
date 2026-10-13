# Durable Waits and External Signals

Durable waits let a workflow pause for minutes, days, or an unknown amount of
time without keeping a Bun worker or JavaScript promise alive. The wait is
stored by the Rust runtime. If WOML stops, the machine restarts, or the runtime
is deployed again, the same run can continue from the same wait boundary.

Use them when the next action depends on time or on something outside the
current process: a CI build, security scanner, cloud operation, maintenance
window, payment callback, or another long-running system.

## Choose the right wait

| Need | WOML element | Completion result |
| --- | --- | --- |
| Pause for a duration | `<wait duration="10m" />` | `status`, `scheduledAt`, `resumedAt` |
| Resume at an instant | `<wait-until at="..." />` | `status`, `scheduledAt`, `resumedAt` |
| Wait for a callback | `<wait-for signal="..." correlation="..." />` | `status`, `signal`, `correlation`, `payload`, `receivedAt` |

All three are flow items inside `<steps>`. Their `id` is a stable durable
identity and becomes their result key under `context.steps`.

## Wait without occupying a worker

```xml
<step id="prepare">
  <script>
    return { readyAt: new Date(Date.now() + 60_000).toISOString() };
  </script>
</step>

<wait-until id="window" at="{{context.steps.prepare.readyAt}}" />

<step id="continueWork">
  <script>
    return { resumedAt: context.steps.window.resumedAt };
  </script>
</step>
```

When execution reaches `window`, Rust first records the wait and its deadline,
then releases execution resources. A scheduler later settles the wait exactly
once and resumes the same run. A past `at` value still records and settles the
durable boundary; it does not silently skip it.

Try the small timer example:

```bash
woml run examples/durableWaits/timer.woml
```

Press Enter. The run pauses for two seconds and then continues.

## Wait for an external system

The workflow identifies the external operation before it waits:

```xml
<step id="dispatchBuild">
  <script>
    return { buildId: crypto.randomUUID() };
  </script>
</step>

<wait-for
  id="buildResult"
  signal="ci.build.succeeded"
  correlation="{{context.steps.dispatchBuild.buildId}}"
  timeout="1h"
  on-timeout="fail"
>
  <schema>
    {
      "type": "object",
      "required": ["commit", "url"],
      "properties": {
        "commit": { "type": "string" },
        "url": { "type": "string" }
      },
      "additionalProperties": false
    }
  </schema>
</wait-for>
```

The callback must match all of these values:

- the run ID;
- the wait ID;
- one accepted signal name;
- the exact correlation value;
- the declared payload schema;
- a unique idempotency key.

That prevents a valid callback for one job from waking another job.

## Configure authenticated ingress

External signal HTTP ingress is disabled unless the project has a dedicated
token:

```bash
woml secrets set WOML_SIGNAL_TOKEN
```

Use a random high-entropy value. WOML resolves it from the secret store when a
loaded workflow contains `<wait-for>`. The token is never printed. The external
system needs the same value in its own secret manager so it can send the bearer
header.

Start the callback example:

```bash
woml run examples/durableWaits/ciCallback.woml
```

Press Enter and note the printed run ID. Inspect the pending wait when needed:

```bash
woml get <run-id>
```

The runtime prints the exact HTTP endpoint and a safe request template. A CI
system can deliver a result like this:

```bash
curl --request POST http://127.0.0.1:3000/v1/signals/ci.build.succeeded \
  --header "Authorization: Bearer $WOML_SIGNAL_TOKEN" \
  --header "Content-Type: application/json" \
  --header "Idempotency-Key: github-run-123-attempt-1" \
  --data '{
    "runId": "<run-id>",
    "waitId": "buildResult",
    "correlation": "<run-id>",
    "payload": {
      "commit": "4f42b61",
      "url": "https://ci.example/runs/123"
    }
  }'
```

Use TLS and a reverse proxy before exposing this route outside a trusted host.
The public HTTP endpoint is for applications and remote systems. A local
operator can use the owner-authenticated CLI instead:

```bash
printf '%s\n' '{"commit":"4f42b61","url":"https://ci.example/runs/123"}' > result.json
woml signal ci.build.succeeded \
  --run <run-id> \
  --wait buildResult \
  --correlation <run-id> \
  --id github-run-123-attempt-1 \
  --payload result.json
```

`woml signal` reads the active runtime descriptor and its owner capability. It
does not replace the public callback route for remote integrations.

## Accept more than one result

Use `<accept>` when several signal names can settle one wait:

```xml
<wait-for id="build" correlation="{{context.steps.dispatch.id}}" timeout="1h">
  <accept signal="ci.build.succeeded">
    <schema>{ "type": "object", "required": ["url"] }</schema>
  </accept>
  <accept signal="ci.build.failed">
    <schema>{ "type": "object", "required": ["url"] }</schema>
  </accept>
</wait-for>
```

At most 16 distinct signals may be declared. The first valid settlement wins.
Later signals are rejected as already resolved.

## Idempotency and race behavior

Send one stable idempotency key for one logical callback. Repeating the exact
request returns the original signal identity and does not resume the run twice.
Reusing that key with different content is an identity conflict.

Signal, timeout, and cancellation compete through one durable transaction:

- one outcome wins;
- losing deliveries cannot overwrite it;
- a late callback cannot reopen a completed or cancelled run;
- an early callback can be held for the matching not-yet-registered wait, up to
  the bounded per-run limit;
- timeout `fail` fails the run, while timeout `continue` publishes a
  `timed_out` result and continues.

## Restart and recovery

You may stop the runtime while a run is waiting and start the same workflow
again with the same state path:

```bash
woml run examples/durableWaits/ciCallback.woml --state .woml/state.sqlite
```

Store v15 reconstructs pending waits from durable records and event history.
Verified backup and restore preserve pending waits, and retention never prunes
an active waiting run.

## Security limits

- Signal ingress requires a dedicated bearer token and is rate-limited to 120
  requests per minute per runtime.
- JSON payloads are limited to 1 MiB and depth 32.
- Signal names, identities, correlations, and idempotency keys are bounded.
- Payloads containing known configured secret values are rejected.
- Schemas are evaluated before a signal can settle a wait.
- Inspection exposes digests and bounded metadata, never raw correlations,
  signal payloads, or credentials.

Use Durable Artifacts for large reports or binary results and place only the
artifact reference in the signal payload.

## Inspect and cancel

```bash
woml list --status waiting
woml get <run-id>
woml cancel <run-id>
```

`woml get` shows wait kind, state, accepted signal names, deadline, and age
without exposing payloads. Cancellation durably closes every pending wait;
later callbacks cannot revive the run.

## Current composition boundary

Duration and signal waits execute in the main sequence, choices, forks, and
parallel groups. Per-iteration suspension inside `<for-each>` and a synchronous
`services.workflows.call()` whose child suspends remain fail-closed until those
continuation identities are completed. Do not design production workflows that
depend on those two combinations yet.

For the frozen limits and wire shapes, read the
[Durable Waits v1 protocol](protocols/durable-waits-v1.md).
