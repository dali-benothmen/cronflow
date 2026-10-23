# WOML Streaming Output and Progress v1

**Status:** SOP0–SOP3 implemented on 2026-10-23
**Authority:** Rust core  
**Durability class:** Operational, bounded, and explicitly non-authoritative

## Purpose

Streaming Output and Progress v1 lets a long-running step expose useful work
while it is happening without placing unbounded text in workflow context or in
the durable run-event history. It establishes one replayable operational plane
for process output, script logs, structured progress, truncation notices,
artifact handoff, and stream closure.

The feature has three deliberately separate planes:

| Plane | Owner | Meaning | Can recover workflow state? |
| --- | --- | --- | --- |
| Business events | Durable Rust event store | Decisions, effects, retries, waits, and outcomes | Yes |
| Operational records | Rust Output Store v1 | Redacted logs and progress for people and tools | No |
| Presentation | CLI and future transports | Color, layout, following, and rendering | No |

Deleting `output.sqlite` may remove logs. It must never change whether a run
succeeded, what it returned, whether an effect may retry, or how recovery
continues. Conversely, replaying output never replays workflow work.

## Current-path inventory and migration owner

Every pre-existing path has one migration owner. No path may write directly to
the terminal or to the output database after its listed integration phase.

| Existing path | Current behavior | v1 owner | Integration phase |
| --- | --- | --- | --- |
| `services.process` stdout/stderr | Capture or discard after completion | Process supervisor → Output Store | SOP2 |
| Script `console.*` | Redacted diagnostic text on host stderr | Script Host → trusted runtime writer | SOP3 |
| Execution Progress v1 | Transient retry/for-each callback | Runtime progress adapter | SOP4 |
| Lifecycle progress | Transient reporter | Runtime progress adapter | SOP4 |
| Schedule and interval progress | Transient reporter | Runtime progress adapter | SOP4 |
| Workflow-call progress | Transient reporter | Runtime progress adapter | SOP4 |
| Runtime-policy progress | Transient reporter | Runtime progress adapter | SOP4 |
| Foreground run presentation | Repeated projected snapshots | Operational subscriber + renderer | SOP5 |
| Background runtime log | Process-wide text file | Operational subscriber + compatibility sink | SOP5 |
| Operations Stream v1 | In-memory invalidation stream | Operational Stream v1 gateway | SOP5 |
| Current `--logs` | Re-fetches run presentation | Cursor replay followed by live subscription | SOP5 |
| Backup | Durable state and artifacts | Output excluded unless explicitly exported | SOP6 |
| Prune and retention | Durable-run policies | Output retention authority | SOP6 |
| Observability | Aggregated runtime facts | Body-free output measurements | SOP7 |

## Frozen records

Operational Record v1 is the only persisted public record. Its fields are flat
so JSON Lines remain easy to inspect and pipe. Rust assigns `deploymentId`,
`sequence`, `occurredAt`, and the complete execution identity.
An author-controlled worker can submit only the bounded body associated with a
trusted in-flight invocation.

Record bodies form a closed union:

- `output`: redacted UTF-8 text from `stdout`, `stderr`, `script`, or `runtime`;
- `progress`: a latest-value progress lane identified by `progressId`;
- `truncated`: an explicit count and reason for output that was not retained;
- `artifact`: a body-free reference to overflow stored by Durable Artifacts;
- `closed`: the terminal state of the run's operational stream;
- `gap`: a synthesized replay notice; it is never accepted by append.

Unknown fields and unknown variants fail validation. JSON is the public
encoding, so invalid UTF-8 and non-finite JSON numbers have no representation.
Rust independently rejects non-finite in-memory progress values.

## Ordering and cursors

One output database belongs to exactly one `deploymentId`. Its sequence is:

- unsigned, positive, and strictly increasing;
- allocated and inserted in the same immediate SQLite transaction;
- shared by all runs in that deployment;
- never reused after restart or pruning;
- the sole public cursor value.

`after=0` means “from the earliest retained record.” A page returns records in
ascending sequence order and a `nextAfter` equal to the last returned record,
or the effective cursor when no record is returned.

Run-scoped replay always applies the run filter in SQLite. A cursor never
widens a caller from one run to another. Global replay is an authenticated
local-operator operation.

Retention preserves a `prunedThrough` watermark globally and per pruned run.
When a requested cursor is older than that watermark, the page contains an
explicit `gap`. Consumers must show the gap and continue from the retained
boundary; they must not pretend the replay is complete.

## Output Store v1

The default location is `.woml/output.sqlite`. A custom durable-state location
derives an adjacent output-store location unless an operator configures one
explicitly. The store is lazy: a workflow that emits no streaming output or
structured progress does not create or open it.

The store uses a private regular file, rejects symbolic links and Windows
reparse points, rejects hard-linked database files on Unix, uses full SQLite
synchronous commits, verifies `PRAGMA quick_check`, binds its deployment ID,
and rejects future schema versions. Version 1 begins the additive migration
line; destructive or reinterpretive migrations require a new store contract.

The normalized tables contain:

- immutable encoded records and indexed, body-free routing columns;
- a latest-progress projection keyed by run, node, attempt, and progress ID;
- bounded per-run record and byte accounting;
- active/terminal run state for retention protection;
- per-run and deployment pruning watermarks.

No index contains output text, progress messages, arguments, environment
values, secrets, artifact paths, or provider payloads.

## Frozen budgets

| Resource | v1 value |
| --- | ---: |
| Decoded process output chunk | 16 KiB |
| Script log/progress frame | 4 KiB |
| Persisted output text field | 16 KiB |
| Replay page records | 1,000 |
| Replay page encoded record bytes | 1 MiB |
| Followers per runtime | 8 |
| Default inline output per run | 16 MiB |
| Maximum configurable inline output per run | 256 MiB |
| Default output retention | 7 days |
| Default total output-store budget | 1 GiB |
| Persisted progress rate | At most 4 snapshots/second/lane |

Policy may lower a budget but cannot exceed the protocol ceiling. Overflow is
never silent: the selected policy emits `truncated`, persists an artifact, or
fails the producing operation.

## Process v2

Process v1 remains byte-for-byte compatible. Process v2 adds exactly one
top-level request field, `stream: boolean`. Existing `output.stdout`,
`output.stderr`, and `output.maxBytes` keep controlling the bounded returned
value. Streamed bytes are redacted across chunk boundaries before persistence
or publication. The final result adds `streamed`, start/end sequence,
truncation, and optional artifact metadata without duplicating complete output.

The Rust process supervisor owns both pipe drains. It preserves stdout/stderr
source identity, incrementally reconstructs UTF-8, replaces invalid byte
sequences safely, and applies stateful secret redaction before append. Capture
and streaming are independent, and the supervisor keeps draining after the
stream budget is exhausted so a noisy child cannot block on a full pipe.

Process Policy v1 accepts the optional `limits.maxStreamBytes` limit. Existing
policies default to 16 MiB; operators may lower it or raise it through the
reviewed 256 MiB protocol ceiling without granting any new executable,
working-directory, environment, or shell authority. Store activation occurs
before spawn. A post-spawn output-authority failure terminates the owned process
tree and returns an ambiguous capability failure rather than silently losing
requested output.

## Script Host v12

Script Host v12 adds strict `script_log`, `progress_report`, and `progress_ack`
frames with monotonically increasing invocation-local frame sequences. Frames
carry only the host request ID plus bounded content. They cannot carry
workflow, run, node, attempt, or invocation identity; the trusted runtime
obtains that identity from the already-authorized in-flight request. Existing
v11 control-message bodies retain their meaning with protocol version 12.

Scripts and imported modules receive the same deeply frozen
`services.progress.report()` facade. Calls are awaited until Rust validates and
accepts the frame. `console.log`, `console.info`, `console.warn`, and
`console.error` become structured script records in v12; v11 retains its exact
stderr behavior.

The worker redacts known WOML secrets and materialized artifact paths before a
frame crosses the worker boundary. Rust repeats redaction before persistence,
binds the frame to the trusted active invocation, and rejects late,
cross-invocation, oversized, invalid, or post-cancellation frames. Progress is
coalesced per attempt and progress ID at no more than four persisted snapshots
per second, while the newest accepted value is force-persisted before attempt
closure. These records never enter Capability Call history or the durable run
event stream.

## Failure behavior

Stable output-plane errors begin with:

- `WOML_OUTPUT_STORE_PATH_UNSAFE`;
- `WOML_OUTPUT_STORE_UNAVAILABLE`;
- `WOML_OUTPUT_STORE_CORRUPT`;
- `WOML_OUTPUT_STORE_VERSION_UNSUPPORTED`;
- `WOML_OUTPUT_STORE_DEPLOYMENT_MISMATCH`;
- `WOML_OUTPUT_RECORD_INVALID`;
- `WOML_OUTPUT_CURSOR_INVALID`.

If output is optional, an output-plane failure degrades observability and
produces a bounded operator diagnostic; it does not rewrite business history.
If a workflow explicitly requests a fail-on-output-loss policy, the producing
operation fails through the normal durable capability path. Ambiguous output
persistence never implies an ambiguous business effect.

## Compatibility and non-goals

- Compiled Workflow Models v1 through v20 do not change.
- Run Event v19 and Durable Store v19 do not change.
- Capability Call v1 does not change.
- Process v1 remains valid and retains capture/discard behavior.
- Output Store v1 is not a message broker, workflow-event store, audit log,
  secret vault, arbitrary file store, or exactly-once external delivery API.
- v1 does not offer global multi-deployment ordering.
- v1 does not persist raw terminal control sequences for trusted replay.
- v1 does not include output in normal workflow backup automatically.

## Security invariants

The detailed threat model is in
[`streaming-output-and-progress-v1-threat-model.md`](../streaming-output-and-progress-v1-threat-model.md).
The central invariants are: redact before persistence, sanitize before terminal
rendering, bound every producer and consumer, trust only Rust-assigned
identity, fail closed on unsafe storage, and keep output replay operational
rather than executable.
