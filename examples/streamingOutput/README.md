# Streaming Output and Progress

[`terraformRelease.woml`](terraformRelease.woml) is a production-shaped example
of WOML's operational output plane. It creates and preserves an immutable
Terraform plan, waits for two reviewers, owns a fenced deployment lease,
streams `terraform apply`, verifies the rollout with `kubectl`, and streams a
corrective Terraform operation if the saga must compensate.

The output is useful to operators but is never workflow state. Deleting
`.woml/output.sqlite` removes retained operational history without changing a
run result or replaying work.

## Prepare the example

1. Copy [`process-policy.example.json`](process-policy.example.json) and replace
   both executable paths with absolute trusted paths.
2. Review [`artifact-policy.example.json`](artifact-policy.example.json). Its
   explicit `operationalOutput` grant is what permits a truncated complete
   transcript to become a durable artifact.
3. Create `.woml/artifact-input/production.tfplan` and
   `.woml/artifact-input/recovery.tfplan`, or provide a safe
   `recoveryPlanPath` in the trigger payload.
4. Configure a reviewer directory with the `production-owner` and
   `security-owner` roles. The format is demonstrated in
   [`../richApprovals/reviewer-directory.example.json`](../richApprovals/reviewer-directory.example.json).

Never point the sample policy at a production binary or directory until its
arguments, environment, reviewers, artifact roots, timeouts, compensation, and
Infrastructure Policy have been reviewed for that environment.

## Run and follow

```bash
woml run examples/streamingOutput/terraformRelease.woml \
  --process-policy examples/streamingOutput/process-policy.example.json \
  --artifact-policy examples/streamingOutput/artifact-policy.example.json \
  --reviewer-directory examples/richApprovals/reviewer-directory.example.json \
  --background

woml logs streamed-terraform-release --follow
```

Use `--phase forward` for the plan/apply/verification stream and
`--phase corrective` for compensation output. Ctrl+C detaches the viewer; it
does not cancel the run.

```bash
woml logs <run-id> --phase corrective --no-follow
woml get <run-id> --json
woml prune --before 7d --dry-run --json
```

## Smaller authoring examples

Stream a supervised process while keeping its returned value bounded:

```js
const result = await services.process.run({
  executable: "bun",
  args: ["test"],
  stream: true,
  timeout: "5m",
  output: { stdout: "capture", stderr: "capture", maxBytes: 65536 }
}, { name: "project-tests" });
```

Publish structured progress without creating durable workflow events:

```js
await services.progress.report({
  id: "files",
  current: completed,
  total: files.length,
  unit: "files",
  message: "Validating generated files"
});
```

See [Streaming Output and Progress](../../docs/woml-streaming-output-and-progress.md)
for storage, redaction, retention, replay, and security behavior.
