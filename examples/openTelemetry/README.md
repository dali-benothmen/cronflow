# OpenTelemetry Export

This directory is the copy-ready local environment for the
[WOML OpenTelemetry guide](../../docs/woml-opentelemetry.md).

It contains:

- an OpenTelemetry Collector receiving OTLP/HTTP on `127.0.0.1:4318`;
- Jaeger for trace exploration on `http://127.0.0.1:16686`;
- Prometheus for metric exploration on `http://127.0.0.1:9090`;
- Collector debug output for safe structured logs;
- Runtime Configuration v2 with every OpenTelemetry signal enabled; and
- a production-style parent/child workflow pair demonstrating process, HTTP,
  approval, durable-wait, Workflow Call, compensation, and cleanup spans.

Start the local observability services:

```bash
docker compose -f examples/openTelemetry/docker-compose.yml up -d
```

Send a safe workflow run to them:

```bash
OTEL_SERVICE_NAME=woml-local \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
woml run examples/helloWorkflow.woml \
  --config examples/openTelemetry/woml.runtime.json
```

Press Enter to run the workflow, then inspect `woml-local` in Jaeger and query
`{__name__=~"woml_runtime_runs.*"}` in Prometheus. Safe structured logs are
visible with:

```bash
docker compose -f examples/openTelemetry/docker-compose.yml logs -f collector
```

The `release-operator.woml` example is deliberately production-shaped rather
than immediately executable. Replace `status.example.test`, configure the
process and infrastructure policies for your project, and configure a reviewer
directory before using it. Check the complete workflow family without making
network or process calls:

```bash
woml check examples/openTelemetry/ \
  --config examples/openTelemetry/woml.runtime.json
```

Stop and remove the local containers with:

```bash
docker compose -f examples/openTelemetry/docker-compose.yml down
```
