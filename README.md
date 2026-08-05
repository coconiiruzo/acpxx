# acpxx / agentmux

`agentmux` is a handle-first local broker that operates Codex, Claude, Grok, and Cursor through
stable ACP v1. The Rust package is named `acpxx`; the distributed executable is `agentmux`.

Version 2 removes central provider-version authorization. A provider version is not rejected merely
because agentmux has never seen it. Runtime admission is based on the built-in `ProviderDriver`,
local executable safety, optional exact user assertions, ACP protocol negotiation, required
capabilities, and the configured permission policy.

## Product shape

```text
CLI / automation
  -> Unix Domain Socket (IPC v2)
  -> local broker + Agent/Run actors
  -> official ACP Rust SDK
  -> owned provider process tree
  -> Grok | Cursor | Codex adapter | Claude adapter
```

The public control operations are `spawn`, `send`, `followup`, `interrupt`, and `list`.
Observation uses `events`, `wait_run`, `wait_any`, and `wait_all` without polling.

- `AgentId` is a logical worker; `RunId` is one execution. IDs are UUIDv7 and are never derived
  from a path, name, provider, or process.
- `send` only appends to the Agent mailbox. It creates no Run and does not steer an active turn.
- `followup` creates the next Run on the same live ACP process, transport, and session.
- continuity loss never creates a replacement session or replays a transcript.
- Run state is exactly `queued`, `running`, `succeeded`, `failed`, or `interrupted`.

## Runtime compatibility

```text
resolve/canonicalize executable
  -> regular-file, owner, mode, executable checks
  -> SHA-256 + file identity observation
  -> best-effort version/component probes
  -> optional exact local assertions
  -> file-identity recheck immediately before spawn
  -> ACP v1 initialize + capability checks
  -> session/new + prompt
```

Version probe failure or an unfamiliar/non-semver version is diagnostic information when no
assertion is configured. The launch artifact safety checks, an assertion mismatch, an artifact
replacement, ACP negotiation failure, or a missing required capability are hard failures.
`RunReceipt.provider_identity` records what actually ran without claiming trust or support.

Past real-provider measurements are in [TESTED_PROVIDERS.md](TESTED_PROVIDERS.md). That document is
reference data only and is never read by the runtime.

## Install and run

Requirements are macOS arm64, Rust 1.96+ for source builds, and at least one separately installed
and authenticated provider or ACP adapter. agentmux never installs or updates one.

```bash
cargo build --locked
chmod 600 ~/.config/agentmux/providers.toml
target/debug/agentmux config migrate --check
target/debug/agentmux doctor --json
target/debug/agentmux serve --provider-limit codex=2
target/debug/agentmux spawn --profile codex-default --cwd /repo \
  "Investigate the failing tests"
target/debug/agentmux wait RUN_UUID
target/debug/agentmux followup AGENT_UUID --after RUN_UUID \
  "Run the remaining tests"
```

Inspect a profile without creating an ACP session:

```bash
agentmux provider inspect codex-default --json
```

Provider configuration uses final schema v2. Assertions are optional and exact:

```toml
schema_version = 2

[profiles.grok-default]
provider = "grok"
executable = "/absolute/path/to/grok"
permissions = "deny"

[profiles.grok-default.assertions]
version = "0.2.118"
launch_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

Delete the assertions table to allow newer local versions through the normal runtime negotiation
path. Permission requests are denied by default; `--permissions allow-all` is an explicit policy
choice and is independent of provider version.

## Security and persistence

- provider environment names are allowlisted and secret values are redacted;
- ACP stdout is protocol-only; stderr, events, output, IPC frames, and metadata files are bounded;
- child and grandchild processes are owned and reaped without process-name searches;
- SQLite schema v3 stores metadata, terminal receipts, and observed identity only;
- prompts, assistant output, reasoning, session secrets, and environment values are not persisted;
- broker restart terminates prior active Runs as `host_restarted` and marks continuity lost.

## Documentation

- [Product contract](PRODUCT_CONTRACT.md)
- [Architecture](docs/architecture.ja.md)
- [Provider setup](docs/provider-setup.md)
- [CLI](docs/cli-reference.md) and [Rust API](docs/rust-api.md)
- [Conformance](CONFORMANCE.md) and [v2 Definition of Done](V2_DEFINITION_OF_DONE.md)
- [Installation](docs/installation.md), [troubleshooting](docs/troubleshooting.md),
  [upgrade](docs/upgrade-policy.md), and [release](docs/release.md)
- [v2.0.0 release notes](docs/release-notes-v2.0.0.md)
- [Security](SECURITY.md) and [performance methodology](PERFORMANCE.md)
- [ADR-0006](docs/adr/0006-runtime-negotiated-provider-compatibility.md)

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Authenticated provider and real-process audits are ignored/manual because they use local login
state and may consume model quota. See [CONFORMANCE.md](CONFORMANCE.md).

Apache-2.0.
