# Export WOML Telemetry with OpenTelemetry

WOML can export traces, metrics, and safe structured logs through OTLP over
HTTP. Export is optional and operational: the Rust engine remains the source of
workflow truth, while an unavailable collector can only degrade telemetry. It
cannot reject a trigger, fail a step, change readiness, or change a run result.

This guide starts with a local collector, explains what WOML exports, and ends
with the controls needed for production.

## Run the local stack

The repository includes a pinned OpenTelemetry Collector, Jaeger, and
Prometheus stack. The Collector receives WOML telemetry, forwards traces to
Jaeger, exposes metrics for Prometheus, and prints safe log records through its
debug exporter.

```bash
docker compose -f examples/openTelemetry/docker-compose.yml up -d
```

The services bind only to loopback:

| Service | Address | Purpose |
| --- | --- | --- |
| OTLP/HTTP | `http://127.0.0.1:4318` | WOML export destination |
| Collector health | `http://127.0.0.1:13133` | Local collector health |
| Jaeger | `http://127.0.0.1:16686` | Trace search and span inspection |
| Prometheus | `http://127.0.0.1:9090` | Metric queries |

The configuration follows the official
[Collector Docker setup](https://opentelemetry.io/docs/collector/install/docker/)
and uses only standard OTLP/HTTP pipelines. Jaeger and Prometheus are CNCF
projects; neither changes WOML's export format.

## Send the first run

OpenTelemetry settings live in runtime configuration and environment—not in
`.woml` source. Start a familiar workflow with the example Runtime
Configuration v2 document:

```bash
OTEL_SERVICE_NAME=woml-local \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
woml run examples/helloWorkflow.woml \
  --config examples/openTelemetry/woml.runtime.json
```

Press Enter once. In Jaeger, choose `woml-local` and open the run trace. In
Prometheus, query:

```promql
{__name__=~"woml_runtime_runs.*"}
```

The Collector normalizes OpenTelemetry names for Prometheus and may add
counter or unit suffixes. This query works across those standard translations.

View the correlated structured logs with:

```bash
docker compose -f examples/openTelemetry/docker-compose.yml logs -f collector
```

These logs contain fixed WOML event names and reviewed metadata. They do not
contain script output, process output, trigger payloads, step results, approval
comments, HTTP bodies, artifact contents, or secret values.

## Runtime configuration

The local example uses this supported shape:

```json
{
  "schemaVersion": 2,
  "observability": {
    "health": true,
    "metrics": true,
    "openTelemetry": {
      "enabled": true,
      "protocol": "http/protobuf",
      "traces": true,
      "metrics": true,
      "logs": true,
      "samplingRatio": 0.1,
      "exportIntervalMs": 10000,
      "exportTimeoutMs": 5000,
      "shutdownTimeoutMs": 5000,
      "maxQueueSize": 2048,
      "maxExportBatchSize": 512,
      "propagation": true
    }
  }
}
```

`maxQueueSize` bounds host and SDK memory. `maxExportBatchSize` bounds each
handoff. Export, flush, and shutdown have explicit deadlines. Repeated failures
open a short circuit; batches arriving while it is open are dropped visibly
instead of allowing an outage to consume unbounded memory.

Sampling applies to traces. Metrics and logs retain their independent enable
switches. A value of `1` samples every locally created trace; production
deployments normally use a smaller ratio.

## Standard environment variables

WOML supports this reviewed standard subset:

| Variable | Meaning |
| --- | --- |
| `OTEL_SDK_DISABLED` | Disable all OpenTelemetry export. |
| `OTEL_SERVICE_NAME` | Set the bounded `service.name`. |
| `OTEL_RESOURCE_ATTRIBUTES` | Add allowlisted deployment/cloud attributes. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Set the common OTLP/HTTP base endpoint. |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | Override the complete trace endpoint. |
| `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` | Override the complete metric endpoint. |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` | Override the complete log endpoint. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | Must resolve to `http/protobuf`. |
| `OTEL_EXPORTER_OTLP_HEADERS` | Inject common exporter headers at startup. |
| `OTEL_EXPORTER_OTLP_*_HEADERS` | Inject signal-specific exporter headers. |
| `OTEL_TRACES_EXPORTER` | Enable or disable trace export. |
| `OTEL_METRICS_EXPORTER` | Enable or disable metric export. |
| `OTEL_LOGS_EXPORTER` | Enable or disable log export. |
| `OTEL_TRACES_SAMPLER` | Select the supported sampler. |
| `OTEL_TRACES_SAMPLER_ARG` | Set the ratio sampler argument. |

Environment overrides non-sensitive JSON configuration. A common endpoint gets
the standard `/v1/traces`, `/v1/metrics`, and `/v1/logs` paths. Per-signal
endpoints are used exactly as supplied, matching the
[OTLP exporter specification](https://opentelemetry.io/docs/specs/otel/protocol/exporter/).

## Inject authentication without storing it

Exporter header values are accepted only through environment variables. Do not
put tokens in `woml.runtime.json`:

```bash
export OTEL_EXPORTER_OTLP_HEADERS='authorization=Bearer%20REPLACE_AT_DEPLOYMENT'
```

In production, inject this variable from the service manager, container secret,
or orchestrator secret. WOML resolves it only when the exporter starts. Health,
logs, errors, inspector output, durable history, backup data, and resolved
configuration expose header names but never values.

Endpoints reject user information, query strings, and fragments. Use HTTPS for
remote collectors and terminate trust with the operating system's reviewed CA
store or a collector-side proxy.

## Understand the trace

One WOML run owns one deterministic trace. Settled operations become completed
spans derived from durable timestamps; a workflow waiting for hours does not
retain an in-memory span for hours.

The production-shaped example under `examples/openTelemetry/` demonstrates:

- a supervised local `git` process;
- a managed HTTP dependency check with W3C propagation;
- a saga compensation and mandatory cleanup;
- a rich human approval without exporting its proposal or comments;
- a durable external-signal wait;
- a synchronous child Workflow Call; and
- a final run outcome.

Validate it without executing an external effect:

```bash
woml check examples/openTelemetry/ \
  --config examples/openTelemetry/woml.runtime.json
```

Before executing it, replace `status.example.test`, authorize `git` through the
process policy, configure infrastructure policy and reviewer identities, and
route the `release.deployed` signal from your deployment system.

Inbound valid W3C context becomes a link to WOML's own stable run trace. WOML
never lets input choose its internal trace identity. Managed `services.http`
requests receive a WOML-owned `traceparent`; author-supplied `traceparent`,
`tracestate`, and `baggage` cannot override it. Native `fetch`, imported HTTP
clients, and process environments are intentionally outside automatic v1
propagation.

## Operate and diagnose the exporter

WOML readiness remains `ok` during an exporter outage. Authenticated detailed
health and the Runtime view in `woml inspect` report:

- queue size, capacity, and utilization;
- completed exports and flushes;
- failures, timeouts, and deterministic drops;
- circuit state and next retry time; and
- last successful export, failure, and flush timestamps.

```bash
woml inspect --state ./.woml/state.sqlite
```

Prometheus also exposes label-free exporter self-metrics such as:

```promql
woml_exporter_queue_utilization
woml_exporter_failures_total
woml_exporter_dropped_total
woml_exporter_circuit_open
```

Diagnostics use stable `WOML_OTEL_*` codes, discard collector response bodies,
and are rate-limited. A recovery closes the circuit and returns the component
to healthy without replaying or changing workflow history.

Common checks:

1. `curl http://127.0.0.1:13133/` confirms the local Collector is healthy.
2. Verify the endpoint is OTLP/HTTP, not OTLP/gRPC.
3. Verify a common endpoint omits `/v1/traces`; WOML adds signal paths.
4. Verify a per-signal endpoint includes its complete receiving path.
5. Check `woml inspect` for queue pressure, timeouts, and circuit state.
6. Check collector logs without copying remote response bodies into WOML logs.

## Privacy and durability boundary

OpenTelemetry is best-effort operational data, not a backup or an audit log.
Export may be sampled, retried, duplicated after recovery, or dropped during an
outage. Durable WOML events, artifacts, waits, approvals, locks, compensation,
and run inspection remain authoritative.

Allowed telemetry is restricted to stable identities, operation categories,
statuses, timestamps, bounded durations/counts, relationship IDs, and stable
error-code families. Metric dimensions cannot contain run IDs, workflow values,
node IDs, invocation IDs, URLs, customer IDs, tenant IDs, or arbitrary author
labels.

Stop the local stack when finished:

```bash
docker compose -f examples/openTelemetry/docker-compose.yml down
```

The exact compatibility and security boundary is frozen in
[OpenTelemetry Export v1](protocols/opentelemetry-export-v1.md) and the
[OpenTelemetry threat model](opentelemetry-export-v1-threat-model.md).
