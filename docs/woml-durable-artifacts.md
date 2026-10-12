# Durable Artifacts

Durable Artifacts let a workflow preserve real files without putting their bytes, local paths, or backend URLs into workflow context. Use them for Terraform plans, test reports, database dumps, diagnostic bundles, generated media, and any other file that must survive a runtime restart and remain tied to its exact SHA-256 digest.

## When to use artifacts

Use `services.artifacts` when the file itself is part of the workflow's durable evidence or handoff. The service streams bytes into an operator-selected backend and returns a small immutable reference.

| Need | Use |
| --- | --- |
| Pass a small JSON value to the next step | `context.steps` |
| Remember a small value across runs | `services.state` |
| Temporarily accelerate repeated reads | `services.cache` |
| Manage application-style keys and objects | `services.storage` |
| Preserve a file with digest, provenance, retention, backup, and recovery | `services.artifacts` |

Artifact bodies never enter context, event history, terminal output, logs, metrics, or normal diagnostics. A reference is not authorization: v1 access is limited to the producing run and its lifecycle hooks.

## Store a file

The file must be beneath one of the read roots approved by the operator's Artifact Policy v1.

```xml
<step id="preservePlan" name="Preserve Terraform plan">
  <script>
    const artifact = await services.artifacts.put(
      ".woml/artifact-input/release.tfplan",
      {
        name: "release-terraform-plan",
        contentType: "application/octet-stream",
        retention: "30d"
      }
    );
    return { artifact };
  </script>
</step>
```

The returned value contains only `id`, `name`, `size`, `sha256`, and `contentType`. The artifact ID represents logical provenance; SHA-256 identifies the exact bytes.

## Verify and consume it

`head` verifies the logical record and backend bytes. `materialize` creates a private file for the current step attempt. WOML removes that path when the attempt settles, so return the artifact reference—not the path—to later steps.

```xml
<step id="verifyPlan" name="Verify preserved plan">
  <script>
    const artifact = context.steps.preservePlan.artifact;
    const metadata = await services.artifacts.head(artifact);
    const path = await services.artifacts.materialize(artifact);
    const verification = await services.process.run({
      executable: "terraform",
      args: ["show", "-json", path],
      timeout: "5m"
    }, { name: "verify-preserved-plan" });
    return {
      artifact,
      integrity: metadata.integrity,
      verified: verification.exitCode === 0
    };
  </script>
</step>
```

## Operator policy

Artifact access is disabled until an operator passes `--artifact-policy`. The policy selects foreground/background use, project and read roots, transfer and quota limits, retention bounds, secret scanning, the private materialization directory, and either a local or S3-compatible backend.

```bash
woml run workflow.woml --artifact-policy ./artifact-policy.json
```

Start from [`examples/durableArtifacts/artifact-policy.json`](../examples/durableArtifacts/artifact-policy.json). Keep policy files operator-owned. Workflows cannot widen them.

The local backend uses content-addressed files below the configured directory. The S3-compatible backend supports ambient credentials or symbolic WOML secrets and requires server-side encryption. SQLite remains the authority for identity, access, provenance, retention, and settlement in both modes.

## Inspect and export safely

`woml get` includes at most 100 artifacts created by that run. It shows name, size, digest prefix, status, expiration, and producing node/attempt. It never shows a body or private path.

```bash
woml get run_... --json
woml artifacts inspect artifact_... --json
```

Export is a separate explicit operator action. It verifies the body, refuses to overwrite an existing destination, and records an audit entry without persisting the destination path.

```bash
woml artifacts export artifact_... \
  --output ./reviewed-plan.tfplan \
  --artifact-policy ./artifact-policy.json
```

WOML does not issue a public or signed URL. If an application needs distribution, make that an explicit, separately authorized workflow step.

## Retention, backup, and recovery

Requested retention must fall inside the operator policy. Expiration makes an artifact eligible for maintenance; it does not delete active or held data immediately.

```bash
woml prune --before 30d --artifact-policy ./artifact-policy.json --dry-run
woml prune --before 30d --artifact-policy ./artifact-policy.json
```

Artifact-aware backup copies and verifies every reachable blob and records a digest-bound inventory in Backup Manifest v2:

```bash
woml backup ./.woml/backups/backup-001 \
  --state ./.woml/state.sqlite \
  --artifact-policy ./artifact-policy.json

woml restore ./.woml/backups/backup-001 \
  --state ./.woml/restored.sqlite \
  --artifact-policy ./artifact-policy.json
```

Maintenance scrubs integrity, reports corruption and orphans, and keeps SQLite metadata authoritative. S3 uploads are staged, conditionally published, and cleaned after interrupted multipart work.

## Metrics and security

When artifacts are enabled, `/metrics` adds the versioned artifact metric family: backend health, transfer count/bytes/duration/failures, corrupt and orphan counts, and live artifact count/bytes. Labels contain only `backend=local|s3`. Metrics never contain names, IDs, paths, digests, URLs, credentials, or body-derived values.

The main security limits are intentional:

- source paths are project-relative and constrained to approved read roots;
- links, junctions, reparse points, device paths, and file replacement races are rejected;
- Rust independently enforces size, quota, timeout, concurrency, retention, access, and integrity rules;
- known resolved secrets are scanned according to policy, but this is not a complete DLP system;
- local files are not automatically encrypted at rest—use filesystem/disk encryption and encrypted backups;
- a copied artifact reference does not grant another run access in v1.

See the [Durable Artifacts v1 protocol](protocols/durable-artifacts-v1.md) and [threat model](durable-artifacts-v1-threat-model.md) for the frozen security boundary.

## Examples

The [`examples/durableArtifacts`](../examples/durableArtifacts/) directory contains local-development, Terraform-plan, test-report, database-backup, and diagnostic-bundle workflows.
