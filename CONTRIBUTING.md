# Contributing

`acpxx` is intentionally phase-gated. Before opening a change, check
[`docs/architecture.ja.md`](docs/architecture.ja.md) and keep the patch inside the
current phase.

## Local checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Provider additions must use ACP, pin an exact tested version, and add black-box
conformance coverage. Do not add CLI-output parsing, implicit downloads,
automatic updates, session replay, or provider fallback.

Commits should be focused and explain any change to public handle semantics,
Run transitions, continuity, or process ownership in an ADR.
