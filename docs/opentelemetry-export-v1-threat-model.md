# OpenTelemetry Export v1 Threat Model

Status: reviewed baseline for OTE0 and OTE1.

## Protected assets

The exporter must protect workflow payloads and results, secrets, source code,
artifact contents, approval material, process arguments and environment,
stdout/stderr, HTTP bodies and sensitive headers, SQL and bind values, signed
URLs, and unrestricted exception messages or stacks.

## Trust boundaries

- Rust is authoritative for workflow transitions and emits only the frozen
  value-free signal contract.
- The Bun host is trusted to validate signals, retain header values in memory,
  and manage bounded SDK providers.
- The collector endpoint, DNS, proxy, network, and observability backend are
  external and may be unavailable, slow, hostile, or misconfigured.
- Runtime JSON is operator-controlled but non-secret. Exporter headers are
  accepted only through the process environment.

## Reviewed threats and controls

| Threat | Required control |
| --- | --- |
| Secret leakage through configuration | Resolved configuration contains header names only; values are re-read privately at startup. |
| Workflow-data leakage | Closed telemetry schemas and explicit attribute allowlists; no arbitrary objects or author labels. |
| Cardinality denial of service or surprise cost | Frozen low-cardinality metric dimensions and bounded span/event attributes. |
| Collector stalls execution | Export runs outside Rust callbacks with bounded queue, timeout, and shutdown deadline. |
| Collector outage changes workflow truth | Export is best effort and cannot affect admission, readiness, or settlement. |
| Endpoint credential exfiltration | Reject URL user info, query, fragments, control characters, and non-HTTP schemes. |
| Redirect/proxy surprises | SDK transport retains normal TLS verification; redirect and proxy behavior must be explicitly tested before release. |
| Header injection | Strict lower-case names, count/byte limits, URL decoding, duplicate rejection, and CR/LF/NUL rejection. |
| Environment changes after preflight | Header-name fingerprint must match at exporter startup or telemetry degrades closed. |
| Telemetry recursion | Exporter diagnostics remain local safe health data and are not recursively exported. |
| Unbounded long-lived spans | Spans are synthesized on completion from authoritative timestamps. |
| Trace-context spoofing | Upstream context is a link, never WOML execution identity or authority; baggage is denied. |
| Crash/replay duplication | Delivery is documented as best effort; stable deterministic identities support consumer deduplication. |

## Fail-closed versus fail-open

Malformed static configuration fails preflight. Malformed runtime signals are
dropped and counted. Missing SDK bundles, network failures, queue overflow,
and flush timeouts degrade telemetry but fail open for workflow execution.

## Deferred review

Before OTE3–OTE6 ship, tests must settle DNS rebinding, proxy inheritance,
redirects, certificate handling, inbound `tracestate`, managed HTTP header
replacement, and backend-specific retry behavior. Those details are not
silently inherited as product promises by OTE1.
