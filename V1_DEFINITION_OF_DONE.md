# v1 Definition of Done

`v1.0.0` may be released only when every item below is satisfied.

## Scope and contract

- [x] The provider set is exactly Codex, Claude, Grok, and Cursor.
- [x] No public custom, catch-all, or maturity-placeholder provider variant exists.
- [x] `spawn`, `send`, `followup`, `interrupt`, and `list` match the frozen contract.
- [x] `wait_run`, `wait_any`, and `wait_all` are event-driven.
- [x] Per-Run event streaming is available as a read-only observation API.
- [x] Run state has exactly five variants.
- [x] Agent and Run identity is independent of path and name.
- [x] `send` neither creates a Run nor steers an active Run.
- [x] One Agent has at most one active Run.

## Continuity and interruption

- [x] `followup` preserves the exact session stamp.
- [x] A continuity loss never creates a replacement session.
- [x] Transcript replay is never treated as continuity.
- [x] `interrupt` targets a Run and is idempotent.
- [x] Failed graceful cancellation escalates through owned process handles.
- [x] Graceful cancel versus forced termination is visible in the receipt.

## Process and broker lifecycle

- [x] Every owned child and grandchild is collected on all tested exit paths.
- [x] No process is found or killed by command name, cwd, or guessed PID.
- [x] A CLI client may exit while its broker-owned Run continues.
- [x] A restarted broker terminates persisted active Runs as `host_restarted`.
- [x] Prior Agents remain visible but continuity-lost after restart.

## Providers

- [x] Grok passes the mandatory conformance suite.
- [x] Cursor passes the mandatory conformance suite.
- [x] Codex passes the mandatory conformance suite.
- [x] Claude passes the mandatory conformance suite.
- [x] Every provider and adapter version is pinned and recorded.
- [x] No provider or adapter is automatically installed or updated.

## Security, quality, and release

- [x] Prompt and output bodies are not persisted by default.
- [x] Secrets are redacted and provider environment variables are allowlisted.
- [x] IPC ownership, mode, protocol handshake, and frame bounds are enforced.
- [x] Race, soak, and chaos gates report no invariant violation, orphan, lost
      notification, unbounded queue, or broker-wide panic.
- [x] The documented performance budget is met on the reference host.
- [ ] A reproducible, signed macOS arm64 artifact, checksum, SBOM, third-party
      license list, and versioned compatibility manifest are published.
- [x] Installation, provider setup, authentication, CLI/API, lifecycle,
      continuity, security, troubleshooting, and upgrade documentation is complete.
