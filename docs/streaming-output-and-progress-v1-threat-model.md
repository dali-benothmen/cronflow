# Streaming Output and Progress v1 Threat Model

## Assets

- resolved secrets and credentials;
- process and script output;
- trusted run, node, attempt, and invocation identity;
- durable workflow history and business outcomes;
- local disk, memory, SQLite writer time, and N-API callback capacity;
- terminal integrity and operator attention.

## Trust boundaries

Workflow JavaScript, child-process output, imported modules, provider payloads,
progress labels, and cursor inputs are untrusted. The Rust runtime, its
verified in-flight invocation identity, and its private Output Store are
trusted only after validation. CLI rendering is a separate boundary: stored
text is data, never terminal instructions.

## Threats and required controls

| Threat | Required control |
| --- | --- |
| Secret split across output chunks | Stateful redaction preserves the maximum secret-overlap window and publishes only finalized redacted bytes. |
| ANSI/OSC terminal injection | Persist normalized text and escape control characters at every terminal renderer. Raw mode is not part of v1. |
| Infinite or high-rate output | Per-chunk, per-run, store, queue, and subscriber budgets; explicit truncation/artifact/failure policy. |
| Forged run or attempt identity | Workers emit request-scoped content only; Rust attaches identity from the authorized invocation. |
| Cross-run cursor probing | Run replay is filtered inside the store query; global replay requires local-operator authority. |
| Symlink, reparse-point, or hard-link attack | Private path checks, safe file creation, Unix `0600`, Windows reparse rejection, and fail-closed open. |
| Slow subscriber blocks execution | Persistence and producer draining do not wait for subscribers; lagging consumers receive a bounded gap/closure. |
| Callback storm overloads N-API | One coalesced wakeup signals available cursor ranges; record bodies are pulled in bounded pages. |
| Output replay confused with workflow replay | Separate database, profiles, APIs, docs, backup rules, and no durable-event append path. |
| Corrupt or future database | Integrity check, exact deployment binding, schema validation, and future-version rejection. |
| Retention deletes active diagnostics | Active-run row is protected; only terminal runs older than policy are candidates. |
| Record text leaks into indexes or telemetry | Index only routing metadata; metrics contain counts and bytes, never bodies. |

## Failure posture

Security checks fail closed. Optional output loss cannot change workflow state.
Consumers must expose truncation and replay gaps instead of silently presenting
partial data as complete.

## Verification status

SOP2 verifies chunk-boundary redaction and process draining. SOP3 verifies
script-host identity confinement. SOP5 verifies slow-consumer and terminal
sanitization behavior. SOP6 verifies bounded retention, governed artifact
rollover, active-run protection, explicit replay gaps, and backup separation.
SOP7 verifies attempt/phase/run isolation, crash interruption, operator
filtering, and production-style infrastructure composition. SOP8 adds the
remaining cross-platform fuzzing, performance gates, packaging, and release
evidence.
