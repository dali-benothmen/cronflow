# OpenTelemetry Export v1 Source Inventory

Status: frozen by OTE0 on 2026-10-23.

This inventory identifies where OpenTelemetry facts may come from. It does not
authorize copying complete values from those sources. The safe output remains
limited by Telemetry Signal v1 and the attribute policy.

| Concern | Authoritative or operational source | OTE mapping decision |
| --- | --- | --- |
| Run admission and settlement | `core/woml-engine/src/event.rs`, `runtime.rs` | Run span and low-cardinality run metrics. Payload/result denied. |
| Step attempts and retries | `event.rs`, `runtime.rs` | One attempt span per durable attempt; retry interval/event. Inputs/outputs denied. |
| Runtime projection and recovery | durable projection/store modules and `runtime.rs` | Completed recovery spans and runtime metrics; no replayed workflow values. |
| Execution progress | native progress callback consumed by `woml-cli/src/cli.ts` | Correlation aid only; never used as authority when a durable fact exists. |
| Operational output | `core/woml-engine/src/operational_output.rs`, `woml-cli/src/operational-output.ts` | Exporter self-health may use bounded counts. Raw output is not exported. |
| Structured runtime logs, health, and Prometheus metrics | `woml-cli/src/runtime-observability.ts` | Safe logs may gain trace correlation; aggregate metrics share frozen meanings. |
| Durable waits and external signals | durable runtime wait records and `runtime.rs` | Completed wait spans. Signal payload and correlation value denied. |
| Approvals | `core/woml-engine/src/rich_approval.rs`, `woml-cli/src/approval-server.ts` | Approval status/timing only. Proposal, evidence, comment, and reviewer data denied. |
| Locks and leases | `core/woml-engine/src/locks.rs` | Acquisition/wait/release spans and aggregate counts. Raw lock keys denied. |
| Compensation and cleanup | durable correction records and `runtime.rs` | Separate corrective spans with phase and stable code. Correction values denied. |
| Workflow calls | `core/woml-engine/src/workflow_calls.rs` | Child relationship or trace link according to call mode. Child payload denied. |
| Process capability | `core/woml-engine/src/process.rs` | Category, duration, exit status, and stable code. Command, arguments, environment, stdout/stderr denied. |
| HTTP capability | `core/woml-engine/src/http.rs` | Reviewed method/status and duration only. Body, query, sensitive headers, and unrestricted URL denied. |
| Events capability | `core/woml-engine/src/events_service.rs` | Category/status/count only. Event payload denied. |
| Notifications | `core/woml-engine/src/notification_runtime.rs` | Provider category and outcome only. Message and destination denied. |
| Infrastructure policy | `core/woml-engine/src/infrastructure_policy_runtime.rs` | Decision class and duration only. Policy source and evaluated values denied. |
| Artifacts | artifact authority/store modules | Operation category, byte-count bucket, status, and duration. Content, preview, path, and signed URL denied. |
| Trigger ingress | trigger admission/runtime modules | Trigger type, admission result, and safe code. Request payload, auth, IP, and arbitrary headers denied. |
| Runtime lifecycle | `woml-cli/src/runtime-activation.ts`, `runtime-control.ts`, `runtime-observability.ts` | Lifecycle/recovery spans and runtime gauges. Local control credentials denied. |

## Explicitly deferred sources

WOML script `console` output, process streams, provider messages, terminal
presentation, artifact previews, and arbitrary workflow labels have no export
mapping in v1. A later contract must opt them in safely; an exporter may not
infer them from existing logs or serialized state.
