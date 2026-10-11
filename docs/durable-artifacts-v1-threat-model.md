# Durable Artifacts v1 Threat Model

**Status:** Frozen security assumptions for DA0 on 2026-10-11  
**Protected boundary:** Untrusted workflow paths and references entering Rust-owned durable artifact storage

## Assets

- Host files readable by the WOML service account
- Artifact bodies and their integrity
- Workflow and run isolation
- Durable provenance and operation settlement
- Backend credentials and private object locations
- Runtime availability, memory, disk, and network capacity
- Backup and retention correctness
- Operator trust in artifact-backed approvals

## Trust boundaries

1. WOML source, trigger payloads, module code, paths, names, content types, and references are untrusted.
2. Bun normalizes author input but is not a path, access, retention, or storage authority.
3. Rust validates the Capability Call and Artifact v1 payload independently.
4. Artifact Policy v1 and Backend Configuration v1 are operator-owned.
5. SQLite owns logical identity, provenance, access, settlement, and retention.
6. Local or S3-compatible storage owns bytes but is not logical authority.
7. The WOML operating-system or cloud identity is the final storage boundary.

## Threats and required mitigations

| Threat | Required mitigation |
| --- | --- |
| Absolute-path or traversal exfiltration | Accept portable relative syntax only; canonicalize beneath approved roots in Rust |
| Symlink, junction, or reparse-point escape | Open and validate the resolved file using platform-aware identity checks |
| File replacement during import | Hold an opened file, stream from that handle, and verify stable identity/size |
| Device, socket, pipe, or directory import | Accept regular files only |
| Memory exhaustion | Stream in bounded chunks; never load an artifact body into protocol JSON |
| Disk exhaustion | Enforce per-artifact, per-run, and store quotas before and during transfer |
| Partial artifact published after crash | Stage privately and expose only after atomic backend and metadata settlement |
| Duplicate logical artifact after retry | Reattach using the stable operation identity and canonical input digest |
| Same operation name with different input | Reject with `WOML_ARTIFACT_OPERATION_IDENTITY_CONFLICT` |
| Cross-run reference forgery | Treat reference as data; authorize against Rust-owned access records |
| Digest substitution | Validate the complete reference and compare it with authoritative metadata |
| Corrupt backend object | Verify size and SHA-256 before successful materialization |
| Backend object mistaken for logical artifact | Require an available SQLite logical record and access authorization |
| Retention deletes a live artifact | Evaluate holds and reachability transactionally before logical or physical deletion |
| Deduplication breaks provenance | Deduplicate physical blobs only; preserve separate logical records |
| Secret appears in body | Scan known resolved secret byte sequences according to policy; never claim complete detection |
| Secret or body appears in logs/events | Persist only bounded IDs, digests, sizes, status, timing, and stable codes |
| Artifact name injects terminal controls | Bound and escape all author metadata at presentation boundaries |
| S3 credential leakage | Resolve symbolic secret names at runtime; never persist credentials or signed URLs |
| Network interception | Require TLS and verify backend responses and checksums |
| Malicious S3-compatible endpoint | Treat backend as untrusted bytes; verify digest and keep metadata authority local |
| Materialized file outlives attempt | Use private attempt directories and startup cleanup |
| Materialized path returned as durable state | Exclude paths from artifact reference, event metadata, inspection, and settlement |
| Arbitrary workflow deletion | Expose no delete API; route deletion through retention authority |
| Unknown future contract accepted | Reject unknown versions and unknown fields at every boundary |

## Security assumptions

- Artifact v1 does not sandbox JavaScript or native processes.
- The runtime account must not have access to unrelated sensitive host files.
- Local artifacts are not transparently encrypted by WOML.
- Object-storage permissions, bucket policy, encryption, and lifecycle policy remain operator responsibilities.
- Known-secret scanning cannot identify encoded, transformed, derived, split, unknown, or short values.
- SHA-256 protects integrity; it does not make a predictable artifact confidential.
- An artifact reference is not a secret and not a bearer token.

## Required DA1 security fixtures

- valid UTF-8 artifact metadata;
- absolute, traversal, backslash, empty-segment, dot-segment, NUL, and unknown-field path rejection;
- future contract-version rejection;
- oversized name and content-type rejection;
- forged reference-field rejection;
- event and settlement metadata rejection for source paths, materialized paths, backend keys, URLs, credentials, body, and secret fields;
- same-run access rule in the behavior matrix;
- stable failure codes with retryability and ambiguity classifications;
- local and S3 backend configuration without embedded credential values.

