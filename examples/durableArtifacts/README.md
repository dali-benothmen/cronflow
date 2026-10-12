# Durable Artifacts examples

These workflows show the difference between temporary workflow data and durable, immutable files. `services.artifacts.put` streams a file into the Rust-owned artifact store and returns a small reference. Later steps use `head` to verify metadata or `materialize` to receive a private, attempt-scoped path.

- `createReport.woml` — local development report generation and verified consumption.
- `terraformPlan.woml` — preserve the exact plan that a human or later workflow will review.
- `testReport.woml` — retain JUnit evidence independently of terminal output.
- `databaseBackup.woml` — retain a scheduled database dump with explicit retention.
- `diagnosticBundle.woml` — collect incident files once and pass a digest-bound reference forward.

Run an example with both security policies:

```bash
woml run examples/durableArtifacts/createReport.woml \
  --process-policy examples/process/process-policy.example.json \
  --artifact-policy examples/durableArtifacts/artifact-policy.json
```

Inspect the run and its bounded artifact metadata:

```bash
woml get <run-id> --json
woml artifacts inspect <artifact-id> --json
```

An operator can explicitly export verified bytes to a new file. WOML does not create a public URL:

```bash
woml artifacts export <artifact-id> \
  --output ./reviewed-report.json \
  --artifact-policy examples/durableArtifacts/artifact-policy.json
```
