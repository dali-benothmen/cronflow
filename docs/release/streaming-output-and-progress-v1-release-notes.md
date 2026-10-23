# Streaming Output and Progress v1 Release Notes

WOML now provides a bounded operational-output plane for live process output,
script logs, and structured progress. Authors can opt a supervised process into
streaming with `stream: true`, publish progress with
`services.progress.report()`, and follow or replay records with `woml logs`.

The release includes:

- Process v2 streaming while preserving Process v1 compatibility;
- Script Host v12 log and progress frames;
- cursor-based replay and reconnect across runtime restarts;
- foreground/background following with source and execution-phase filters;
- one-marker truncation while process pipes continue draining;
- optional Artifact Policy-governed complete-transcript rollover;
- bounded retention integrated with `woml prune`;
- body-free health and budget inspection;
- crash interruption and retry/loop/fork/child/corrective identity; and
- Linux, macOS, and Windows contract, recovery, security, and budget gates.

Operational output is not durable workflow state. It is excluded from ordinary
backup, can be pruned independently, and cannot influence run recovery.
Redaction covers known resolved secrets, including chunk boundaries, but cannot
identify unknown, transformed, or application-sensitive values. Operators must
still grant processes narrowly and protect the local `.woml/` directory.

See [Streaming Output and Progress](../woml-streaming-output-and-progress.md)
for author and operator guidance.
