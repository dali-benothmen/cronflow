# Process v1 Threat Model

**Status:** Frozen security assumptions for SP0  
**Protected boundary:** `services.process.run()` from untrusted workflow input to an OS process

## Assets

- WOML runtime availability
- Host filesystem and credentials
- Resolved workflow secrets
- Durable event integrity
- Correct run and attempt identity
- Operator trust in approvals and diagnostics
- Other processes owned by the WOML service account

## Trust boundaries

1. WOML source and trigger payloads are untrusted inputs.
2. Bun normalizes requests but is not the permission authority.
3. Capability Call v1 is validated independently by Rust.
4. Process Policy v1 is runtime-owned and cannot be modified by workflow code.
5. The operating-system account is the final host-level authority.

## Threats and required mitigations

| Threat | Required mitigation |
|---|---|
| Shell injection | Spawn an executable directly with an argument array; no implicit shell |
| Executable substitution through `PATH` | Resolve policy aliases to canonical approved paths before execution |
| Workflow-selected binary path | Requests carry an alias, never an arbitrary executable path |
| Working-directory escape | Resolve from canonical project root and reject absolute paths, traversal, and symlink escape |
| Ambient credential leakage | Clear environment; inherit only policy-approved names |
| Explicit secret persistence | Never include environment values in events, metadata, metrics, fixtures, or diagnostics |
| Secret printed by child | Redact known exact secret values from presentation; document that transformed secrets cannot be detected perfectly |
| Output memory exhaustion | Drain concurrently, cap capture per stream, and report truncation |
| Pipe deadlock | Drain stdout and stderr concurrently even after capture truncates |
| Fork bomb or excessive concurrency | Enforce process-policy concurrency and generic capability limits; rely on OS resource limits for stronger containment |
| Child survives timeout | Terminate the complete process group or Windows Job Object |
| Child survives host crash | Use OS containment where available; recover unmatched durable starts as ambiguous and never replay |
| Duplicate effect after retry | Classify every command as unsafe; fail closed after ambiguous interruption |
| Sensitive command in history | Persist alias and counts only, never arguments or raw output |
| Malicious argument encoding | Reject NUL and platform-invalid values; pass each argument without shell parsing |
| File descriptor inheritance | Use null/pipe stdio and close unrelated descriptors |
| Interactive credential prompt | No inherited terminal or PTY in Process v1 |
| Detached background process | No detached mode in Process v1 |
| Policy bypass from JavaScript | Enforce policy again inside Rust immediately before spawn |
| Time-of-check/time-of-use executable replacement | Validate canonical identity immediately before spawn; document platform limits and prefer protected executable directories |

## Non-goals

Process v1 does not sandbox arbitrary native code. An approved executable runs
with the WOML runtime operating-system identity and can exercise that identity's
permissions. Production deployments must use a dedicated least-privileged
account, filesystem permissions, containers or OS sandboxing where appropriate,
and network egress controls.

Process v1 also cannot guarantee that arbitrary output never contains a secret.
Known exact injected secret values are redacted from user-facing presentation,
but encoded, transformed, split, or derived values may be unrecognizable.

## Required security tests

- shell metacharacters remain literal arguments;
- forbidden executable aliases never spawn;
- PATH shadowing cannot replace an approved executable;
- `..`, absolute paths, and symlink escapes are rejected;
- unapproved environment variables are absent;
- explicit secrets are absent from durable events and diagnostics;
- stdout and stderr flooding remains bounded;
- child and grandchild processes terminate on timeout and cancellation;
- host interruption never causes automatic replay;
- policy and request unknown fields are rejected;
- Windows, macOS, and Linux packaged releases exercise their native containment path.

