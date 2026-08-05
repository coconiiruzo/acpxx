# Contributing

`acpxx` is intentionally phase-gated. Before opening a change, check
[`docs/architecture.ja.md`](docs/architecture.ja.md) and keep the patch inside the
current phase.

Each implementation phase is a separate epic and pull request. Do not add code,
empty modules, enum variants, compatibility rows, or extension points for a
later phase. A phase is complete only when every documented exit gate passes.

## Local checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
target/release/agentmux benchmark --enforce --json
```

The v1 provider set is Codex, Claude, Grok, and Cursor. Provider integrations
must use ACP, pin an exact tested version, and add black-box conformance
coverage. Do not add CLI-output parsing, implicit downloads, automatic updates,
session replay, provider fallback, or a custom provider plugin surface.

Commits should be focused and explain any change to public handle semantics,
Run transitions, continuity, or process ownership in an ADR.
