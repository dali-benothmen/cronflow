# Secure Process Service v1

WOML now exposes `services.process.run()` for supervised local developer and
infrastructure tools. It ships in the portable `woml-cli` JavaScript package
and every supported native engine package; the Rust engine remains the sole
authority that can start the approved OS process.

## Included

- typed Process v1 request/result API and generated editor declarations;
- explicit foreground `--allow-process` grants and reviewed production policy;
- no implicit shell and literal argument transport;
- canonical executable, working-directory, and environment enforcement;
- Linux/macOS process groups and Windows kill-on-close Job Objects;
- bounded output, known-secret redaction, timeouts, cancellation, and safe
  terminal activity;
- durable operation identity and fail-closed ambiguous recovery;
- cross-language fixtures, adversarial tests, real workflow tests, and
  matching-platform packaged-release containment smoke; and
- user guides, examples, AI skill rules, and migration guidance.

Process execution remains disabled by default. Start with
[Running Local Tools Safely](../woml-process.md), then use the reviewed
[policy template](../../examples/process/process-policy.example.json).
