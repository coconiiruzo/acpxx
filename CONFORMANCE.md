# Provider conformance

Conformance validates the four built-in driver contracts. It does not authorize particular
versions, create a runtime allowlist, or require testing every provider release.

## Deterministic suites

`tests/provider_runtime_compatibility.rs` proves that future/non-semver versions and failed probes
proceed without assertions, local exact assertions work, launch safety and TOCTOU checks are hard
gates, ACP negotiation completes audit identity, and live-Agent continuity remains immutable.

`tests/provider_fault_conformance.rs` runs the same scriptable ACP agent through every driver and
checks crash/protocol classification, no fallback, deny/allow permission behavior, and descendant
cleanup. Other default suites cover event ordering, wait races, interruption, persistence, IPC,
shutdown, chaos, and process supervision.

```bash
cargo test --all-targets
```

## Authenticated real-provider suite

`tests/provider_conformance.rs` checks initialize/authentication/session creation, ordered output,
three same-session turns, output boundaries, cancellation recovery, identity/capability snapshots,
and cleanup using locally authenticated Codex, Claude, Grok, and Cursor installations.

```bash
cargo test --test provider_conformance -- --ignored --test-threads=1
```

`tests/provider_capability_audit.rs` records ACP v1 capability observations. Missing optional
features are reported rather than emulated. `tests/provider_permission_audit.rs` performs isolated
Codex/Claude/Grok mutation allow/deny checks, including a Grok shell-command write. `tests/provider_process_audit.rs` kills a real broker and
requires every owned provider descendant to disappear through the watchdog path.

```bash
cargo test --test provider_capability_audit -- --ignored --test-threads=1
cargo test --test provider_permission_audit -- --ignored --test-threads=1
cargo test --test provider_process_audit -- --ignored --test-threads=1
```

These suites use local login state, may consume quota, never search/kill by process name, and never
trigger automatic installation, update, fallback, or transcript replay. A driver change is ready
only after its deterministic and applicable authenticated/process/permission checks pass. Results
may be noted in `TESTED_PROVIDERS.md` as non-authoritative observations.
