# OpenTelemetry Export v1

Status: frozen by OTE0. Runtime configuration and dormant exporter ownership
are implemented by OTE1. Runtime signal production starts in OTE2.

## Authority boundary

OpenTelemetry is a best-effort operational projection. The Rust engine remains
the sole authority for workflow admission, execution, waits, approvals, locks,
artifacts, correction, cleanup, and settlement. Export failure, duplication,
sampling, or loss cannot alter any workflow result.

Rust will emit reviewed `woml.telemetry-signal/v1` facts only after an
authoritative transition. Bun validates those facts and owns SDK providers,
batching, OTLP network I/O, health, and bounded shutdown. No exporter data is
written back to the durable workflow store.

## Transport and dependencies

Version 1 supports OTLP over HTTP with protobuf encoding. The reviewed SDK
line is pinned exactly in `woml-cli/package.json`:

- `@opentelemetry/api` 1.10.0;
- `@opentelemetry/resources`, `sdk-trace`, `sdk-metrics`, and `sdk-logs` 3.0.0;
- OTLP protobuf trace, metric, and log exporters 0.300.0.

The SDK is bundled into `dist/opentelemetry-sdk.js`. The ordinary CLI bundle
contains only the dormant host and imports that SDK bundle dynamically after
configuration enables a signal.

## Configuration contract

Runtime Configuration v1 is unchanged. Runtime Configuration v2 may contain
`observability.openTelemetry`. The supported environment subset is:

- `OTEL_SDK_DISABLED`;
- `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES`;
- `OTEL_EXPORTER_OTLP_ENDPOINT` plus per-signal endpoint variants;
- `OTEL_EXPORTER_OTLP_PROTOCOL` plus per-signal protocol variants;
- `OTEL_EXPORTER_OTLP_HEADERS` plus per-signal header variants;
- `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, and `OTEL_LOGS_EXPORTER`;
- `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG`.

Environment overrides non-sensitive JSON configuration, which overrides safe
defaults. Only `http/protobuf`, `http`, and `https` are accepted. User info,
query strings, and fragments are rejected. Header values are resolved only at
exporter startup and never appear in resolved configuration, health, logs,
errors, or durable storage. Diagnostics may expose sorted header names.

The global OTLP endpoint receives `/v1/traces`, `/v1/metrics`, or `/v1/logs`.
A per-signal endpoint is used exactly as configured.

## Trace identity and mapping

The cross-language identity algorithm is frozen for OTE2:

- trace ID: first 16 bytes of SHA-256 over `woml:trace:v1\0<run-id>`;
- span ID: first 8 bytes of SHA-256 over
  `woml:span:v1\0<run-id>\0<kind>\0<stable-operation-identity>`;
- an all-zero result is replaced by the same digest with `\0nonzero` appended.

One durable run maps to one trace. Completed runtime intervals map as follows:

| Runtime fact | Span name | Relationship |
| --- | --- | --- |
| Run | `woml.run` | root |
| Step attempt | `woml.step` | run child |
| Retry delay | `woml.retry.wait` | attempt event/child |
| Durable wait | `woml.wait` | run child completed on resolution |
| Approval | `woml.approval` | run child; no proposal or comment |
| Lock/lease | `woml.lock` | attempt child |
| Capability | `woml.capability` | attempt child |
| Workflow call | `woml.workflow.call` | child or link by call mode |
| Compensation | `woml.compensation` | run child |
| Cleanup | `woml.cleanup` | run child |
| Recovery | `woml.recovery` | run child |

Parallel work uses sibling spans. Fire-and-forget workflows and upstream W3C
context use links. Long waits are created as completed spans from durable
timestamps; no SDK span object remains open in memory.

## Attribute and cardinality policy

The exporter accepts identities, operation categories, statuses, timestamps,
durations, bounded counts, and stable `WOML_*` codes. Span attributes are
limited to 32 values of at most 256 characters. Events are limited to 16.
Telemetry Signal v1 accepts only the explicit attributes in its schema.

Metric dimensions are limited to durable status, trigger type, handler or
capability category, corrective phase, and error-code family. Run, invocation,
workflow, node, customer, tenant, URL, and arbitrary author labels are denied
as metric dimensions.

## Metric mapping

Version 1 reserves these low-cardinality instruments for later OTE phases:

- `woml.runtime.runs` counter by terminal status and trigger type;
- `woml.runtime.run.duration` histogram in milliseconds;
- `woml.runtime.queue.duration` histogram in milliseconds;
- `woml.runtime.step.attempts` counter by status and handler category;
- `woml.runtime.step.duration` histogram in milliseconds;
- `woml.runtime.retries` counter by outcome;
- `woml.runtime.active_runs`, `active_waits`, `active_approvals`, and
  `active_locks` gauges;
- `woml.runtime.capability.calls` and `capability.duration` by category/status;
- `woml.exporter.exports`, `failures`, `dropped`, and `queue.utilization`.

## Structured log correlation

Only existing safe WOML structured runtime records are eligible. Correlation
may add `trace_id` and `span_id`. Script/process output, provider messages,
workflow values, and approval contents are not promoted into OTLP logs. Log
export defaults off.

## W3C propagation contract

Version 1 reserves valid inbound `traceparent` and bounded `tracestate` as a
link to a WOML-owned run trace. Invalid context is ignored and counted.
`baggage` is denied. When managed HTTP propagation is enabled, WOML-owned
headers replace author-supplied `traceparent`, `tracestate`, and `baggage`.
Implementation begins after the safe signal boundary is available.

## Lifecycle and failure behavior

The exporter state is one of `disabled`, `starting`, `healthy`, `degraded`,
`flushing`, or `stopped`. Configuration errors fail preflight. SDK import,
collector, export, and flush failures degrade only the telemetry component.
They never close workflow readiness.

Queues and batches are bounded. Shutdown performs a bounded force-flush and
SDK shutdown. A timeout abandons optional telemetry and allows WOML shutdown
to continue.

## Performance budgets

The reviewed OTE0 fixture freezes these initial gates:

- disabled activation median regression: at most 5 ms and 2%;
- disabled short-run median regression: at most 2 ms and 2%;
- disabled steady-state RSS increase: at most 2 MiB;
- disabled exporter timers and sockets: exactly zero;
- main `dist/cli.js` increase: at most 20 KiB;
- separate SDK bundle: at most 2.5 MiB;
- installed package increase: at most 3 MiB.

Performance values are compared against the checked-in pre-OTE baseline; they
are not promises about absolute latency on every machine.
