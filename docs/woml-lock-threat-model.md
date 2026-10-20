# Locks and Leases Threat Model

## Protected assets

The authority protects exclusive ownership, fair admission, monotonic fencing,
bounded wait queues, and the confidentiality of internal lease identities. It
also protects workflow correctness from duplicate delivery, process restart,
late release, timeout races, and stale callbacks.

## Trust boundary

Rust and the durable Store v16 database are authoritative. Bun receives a
bounded public lock context and invokes callbacks, but it cannot grant,
renew, transfer, or release ownership directly. Workflow input, modules, and
scripts are untrusted with respect to the authority.

The initial deployment profile coordinates processes that share one local
SQLite store. It is not a distributed lock across copied databases or hosts.

## Defenses

- strict key grammar and length limits;
- one-owner database constraints and immediate transactions;
- FIFO selection with deterministic tie breaking;
- idempotent acquisition identities;
- per-key and per-store waiting ceilings;
- bounded 250-item recovery batches;
- monotonic fencing high-water;
- renewal compare-and-set against the exact stored expiry;
- cancellation and timeout settlement before another grant;
- secret-free events, metrics, errors, and inspection;
- fail-closed loss when current ownership cannot be proven;
- restore epoch clearing without token reset.

## Residual risks

- A target that ignores fencing can accept a stale owner's request.
- Copying and activating one store on two machines creates split brain.
- A privileged operator can modify SQLite state outside WOML.
- Holding a lock through a slow approval or external wait reduces throughput.
- One-machine authority depends on its clock; future distributed backends need
  authority-owned time and consensus appropriate to their deployment model.
- Locks do not roll back partial effects. Use idempotency and, when available,
  explicit compensation for multi-system operations.

Never place secrets, bearer tokens, private payloads, or customer data in a
lock key. Keys are visible to operators and metrics by design.
