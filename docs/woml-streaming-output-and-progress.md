# Streaming Output and Progress

WOML can expose a long-running process or script while it is still working,
then replay the same bounded operational records after a terminal disconnect or
runtime restart. This is designed for builds, tests, database jobs,
infrastructure plans, deployments, data imports, and other work where waiting
for the final step result hides the information an operator needs.

Streaming output is deliberately separate from durable workflow history:

- run events decide recovery, retries, waits, and business results;
- operational records contain redacted logs and progress for people and tools;
- terminal presentation adds colors and layout without becoming state.

Removing operational output cannot change a workflow result. Following output
cannot rerun a workflow.

## Stream a supervised process

Set `stream: true` on an operator-approved `services.process.run()` request:

```js
const result = await services.process.run({
  executable: "terraform",
  args: ["apply", "-auto-approve", "reviewed.tfplan"],
  cwd: "./infrastructure",
  timeout: "20m",
  stream: true,
  output: {
    stdout: "capture",
    stderr: "capture",
    maxBytes: 65536
  }
}, { name: "apply-reviewed-plan" });

return {
  exitCode: result.exitCode,
  streamTruncated: result.streamTruncated,
  transcript: result.streamArtifact
};
```

`stream` publishes operational records. The `output` object independently
controls the bounded `stdout` and `stderr` returned to the script. A workflow
can stream a noisy command while discarding its returned text, preventing a
large step result from entering durable context.

Process execution remains disabled until an operator grants the executable.
Use `--allow-process` only for reviewed foreground work and a Process Policy
for background or production operation.

## Report structured script progress

Use progress when a script knows the meaning of its work:

```js
for (let index = 0; index < files.length; index += 1) {
  await validate(files[index]);
  await services.progress.report({
    id: "validation",
    current: index + 1,
    total: files.length,
    unit: "files",
    message: `Validated ${index + 1} of ${files.length}`
  });
}
```

`id` selects one progress lane within the current step attempt. `current` and
`total` are finite non-negative numbers, `unit` is a short label, and `message`
is bounded text. At least `current` or `message` is required. Calls are awaited
until the Rust authority validates them.

High-frequency reports update the latest snapshot while persistence is
coalesced to at most four snapshots per second per lane. The newest accepted
value is persisted before the attempt closes. Progress is operational data,
not a durable side effect and not input to later steps.

Script `console.log`, `console.info`, `console.warn`, and `console.error` calls
also enter the operational stream under Script Host v12. Prefer structured
progress for quantities and console messages for bounded diagnostics.

## Follow and replay output

```bash
woml logs <run-id> --follow
woml logs <workflow-id> --tail 200
woml logs <run-id> --after 420 --no-follow
woml logs <run-id> --source stdout,stderr
woml logs <run-id> --phase corrective
woml logs <run-id> --json
```

The older shorthand remains valid:

```bash
woml <run-id|workflow-id> --logs
```

Ctrl+C detaches the viewer. It does not cancel the run or stop the background
runtime. `--after` is a durable deployment cursor; reconnecting from the last
seen cursor avoids duplicate presentation. If retention removed older records,
WOML emits an explicit gap and resumes from the retained boundary.

Use `--phase forward` for ordinary work, `--phase corrective` for saga
compensation and cleanup, or `--phase all` for both. Retry attempts retain
their own node, attempt, and invocation identities. Child workflows retain
their own run streams.

## Retention, truncation, and artifacts

The default local store is `.woml/output.sqlite`. It activates lazily and
retains output for seven days by default. The initial release bounds:

- one decoded process chunk and persisted output field to 16 KiB;
- one script log or progress frame to 4 KiB;
- one replay page to 1,000 records and 1 MiB;
- followers to eight per runtime;
- inline output to 16 MiB per attempt and run by default; and
- the local output store to 1 GiB by default.

When inline output reaches its limit, WOML writes one truncation marker and
continues draining the producer so a full process pipe cannot deadlock the
workflow. It does not write one marker per discarded chunk.

Complete redacted process output can become a Durable Artifact only when the
Artifact Policy explicitly grants `operationalOutput`:

```json
{
  "operationalOutput": {
    "enabled": true,
    "retentionMs": 604800000
  }
}
```

WOML publishes the artifact atomically and returns an immutable Artifact
Reference v1 through `result.streamArtifact`. If policy, quota, secret
scanning, or the backend rejects publication, WOML never falls back to an
unreviewed path; the inline stream remains explicitly truncated.

Plan and execute cleanup with:

```bash
woml prune --before 7d --dry-run --json
woml prune --before 7d
```

Active runs are protected. Pruning is bounded and idempotent. Ordinary
operational logs are excluded from `woml backup`; published Durable Artifacts
follow their own backup and retention contracts.

## What redaction guarantees—and what it cannot

WOML redacts known resolved secret values across process chunk boundaries
before persistence and repeats redaction at the trusted Rust boundary. Script
logs redact known WOML secrets and materialized artifact paths. Stored text is
escaped again before terminal rendering, preventing ANSI and OSC injection.

Redaction cannot recognize a credential WOML does not know, a transformed or
encoded secret, sensitive application data that is not configured as a secret,
or a tool that deliberately exfiltrates data. Avoid secrets in command
arguments, disable unnecessary output, use least-privilege process policies,
and protect `.woml/` with operating-system permissions. Operational output is
not encrypted storage and is not a multi-tenant isolation boundary.

## Failure and recovery behavior

- A requested stream whose output authority cannot start is rejected before
  the process starts.
- Failure after a process starts terminates the owned process tree and fails
  ambiguously rather than pretending no external effect occurred.
- A crash preserves committed records and closes open attempt lanes as
  `interrupted`; WOML never invents missing bytes.
- A disconnected or slow viewer cannot delay the process or run settlement.
- Output-store corruption can remove observability but cannot rewrite durable
  workflow history.

For the wire contracts and exact schemas, see
[Streaming Output and Progress v1](protocols/streaming-output-and-progress-v1.md).
For the adversarial review, see the
[threat model](streaming-output-and-progress-v1-threat-model.md). The complete
Terraform example is under [`examples/streamingOutput/`](../examples/streamingOutput/).
