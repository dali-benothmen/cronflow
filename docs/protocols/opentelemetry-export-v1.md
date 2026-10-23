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

Version 1 exports these low-cardinality instruments from the shared typed
metric facts used by both OTLP and Prometheus encoding:

- `woml.runtime.runs` counter by terminal status and trigger type;
- `woml.runtime.run.duration` histogram in milliseconds;
- `woml.runtime.queue.duration` histogram in milliseconds;
- `woml.runtime.step.attempts` counter by status and handler category;
- `woml.runtime.step.duration` histogram in milliseconds;
- `woml.runtime.retries` counter by outcome;
- `woml.runtime.active_runs`, `active_waits`, `active_approvals`, and
  `active_locks` gauges;
- `woml.runtime.capability.calls` and `capability.duration` by category/status;
- `woml.runtime.propagation.contexts` by `linked` or `invalid` status;
- `woml.exporter.exports`, `failures`, `dropped`, and `queue.utilization`.

## Structured log correlation

Only terminal records derived from the existing safe telemetry facts are
eligible. Their bodies are fixed `woml.<kind>.<status>` values. Correlation
adds the deterministic `trace_id` and `span_id`; attributes remain restricted
to reviewed status/category facts and stable `WOML_*` codes. Script/process
output, provider messages, workflow values, and approval contents are not
promoted into OTLP logs. Log export defaults off and does not alter local
logging.

## W3C propagation contract

Version 1 accepts the exact W3C v00 `traceparent` shape and validates
`tracestate` at no more than 512 bytes and 32 members. A valid inbound context
becomes a link to the WOML-owned run trace through the transient
`woml.telemetry-admission-link/v1` record. Only normalized trace ID, span ID,
and sampled bit cross the native callback; raw headers and `tracestate` are
never retained. Invalid context is ignored without rejecting admission and is
counted as an invalid link record. `baggage` is denied.

When managed HTTP propagation is enabled, WOML derives `traceparent` from the
engine-verified run and attempt identity after capability and infrastructure
policy validation. It replaces author-supplied `traceparent` and removes
author-supplied `tracestate` and `baggage` before transport. Workflow Call and
Workflow Start create the same transient link from the parent attempt to the
child WOML-owned trace. Native `fetch`, imported clients, and process
environment propagation are outside the v1 contract.

## Lifecycle and failure behavior

The exporter state is one of `disabled`, `starting`, `healthy`, `degraded`,
`flushing`, or `stopped`. Configuration errors fail preflight. SDK import,
collector, export, and flush failures degrade only the telemetry component.
They never close workflow readiness.

The native callback hands reviewed facts to a host-owned queue without waiting
for network I/O. Queue capacity and batch size come from the resolved runtime
configuration. When the queue is full, the new record is rejected
deterministically, the drop counter increases, and workflow execution
continues.

Each signal exporter has a bounded whole-attempt deadline, two bounded retry
delays, and a circuit breaker. Three exhausted exports open the circuit for a
short cooldown; records presented while it is open are dropped visibly. The
first request after the cooldown is a half-open probe, and a successful probe
closes the circuit. Collector errors are reduced to fixed `WOML_OTEL_*` codes;
remote response bodies never enter diagnostics. Repeated diagnostics are
rate-limited by code.

Exporter Health v2 adds queue size, capacity, and utilization; export,
failure, timeout, drop, and flush counters; circuit and retry state; and the
last successful export, failure, and flush timestamps. Runtime Health v2,
Runtime Metrics v3, the operations stream, and `woml inspect` expose these
facts. A degraded exporter degrades detailed health but never workflow
readiness.

Shutdown drains the host queue, performs force-flush, and initiates SDK
shutdown within one hard deadline. A timeout abandons optional telemetry and
allows WOML shutdown to continue.

## Deployment experience

The operator guide is [Export WOML Telemetry with
OpenTelemetry](../woml-opentelemetry.md). The copy-ready assets under
`examples/openTelemetry/` pin a Collector, Jaeger, and Prometheus; validate a
Runtime Configuration v2 document; and include a production-shaped workflow
covering process, HTTP, approval, durable wait, Workflow Call, compensation,
and cleanup relationships. Telemetry configuration remains outside `.woml`
source.

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
