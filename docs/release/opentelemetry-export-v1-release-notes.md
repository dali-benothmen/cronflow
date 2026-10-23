# OpenTelemetry Export v1 Release Notes

OpenTelemetry Export v1 adds optional OTLP/HTTP protobuf export to the
long-lived WOML runtime.

## Included

- deterministic run traces and bounded completed-operation spans;
- low-cardinality counters, histograms, and active-operation gauges;
- safe structured logs correlated with trace and span IDs;
- strict W3C context links at supported ingress boundaries;
- WOML-owned propagation through managed HTTP and child workflows;
- independently enabled trace, metric, and log pipelines;
- bounded host and SDK queues, batches, retry, backoff, timeouts, and circuit
  breaking;
- rate-limited sanitized diagnostics and bounded shutdown flush;
- authenticated exporter health, Prometheus self-metrics, operations events,
  and `woml inspect` visibility; and
- a copy-ready Collector, Jaeger, and Prometheus example.

## Compatibility

Runtime Configuration v1 remains valid and OpenTelemetry remains disabled by
default. Runtime Configuration v2 activates the optional exporter. No WOML
language element, compiled model, durable event, or durable-store migration is
introduced. The SDK is loaded lazily only when export is enabled.

Exporter failure never changes trigger admission, workflow readiness, durable
recovery, or business outcomes. OpenTelemetry remains best-effort operational
data rather than durable truth.

## Security and privacy

WOML exports reviewed identities, categories, statuses, timestamps, durations,
counts, relationships, and stable error codes. It denies workflow values,
payloads, results, source, output, approval content, artifact content, HTTP
bodies, process arguments, and secrets.

Exporter credentials are accepted only through reviewed `OTEL_*` environment
variables and are never returned in configuration, health, logs, diagnostics,
inspection, durable storage, or backup data.

See [Export WOML Telemetry with OpenTelemetry](../woml-opentelemetry.md) for
setup and operations.
