# acpxx / agentmux

`agentmux` is the target product name for this repository's handle-first local
broker for coding agents that speak the
[Agent Client Protocol (ACP)](https://agentclientprotocol.com/).

The v2 runtime is one closed control API for exactly **Codex, Claude, Grok, and
Cursor**. The Rust package still uses the repository name `acpxx`; its single
distributed executable is named `agentmux`. Provider execution uses stable ACP
v1. A signed Compatibility Catalog authorizes exact observed identities and
artifact digests independently from the agentmux binary release.

> Status: v2.0.0. Provider compatibility is Catalog-driven, immutable per
> Agent, persisted in receipts, and explicitly updateable without a binary
> release. The four-provider scope and stable ACP v1 wire protocol are unchanged.

The current signed sequence and recommended exact identities are generated in
[Provider compatibility](PROVIDER_COMPATIBILITY.md). A matching version string
without the qualified artifact digest is rejected.

## Why

Coding-agent integrations tend to leak provider process IDs, session IDs, CLI
protocols, or workspace paths into their public APIs. `acpxx` instead exposes
opaque logical handles:

```rust
pub struct AgentHandle {
    pub agent_id: AgentId,
}

pub struct RunHandle {
    pub agent_id: AgentId,
    pub run_id: RunId,
}
```

An `AgentId` identifies a logical worker. A `RunId` identifies one execution.
Names, paths, working directories, and provider names are metadata, never
identity.

## Current vertical slice

```text
spawn
  -> queued Run + handles returned
  -> Agent actor acquires capacity
  -> Driver identity and artifact probes
  -> deny-first signed Catalog resolution
  -> immutable ResolvedProviderLock
  -> grok --no-auto-update agent stdio
  -> ACP initialize (v1)
  -> session/new
  -> session/prompt + streamed session/update collection
  -> persistent same-session follow-up
  -> interrupt/deadline escalation
  -> terminal receipt published to wait_run
```

The implementation uses `tokio::sync::watch`; `wait_run` does not poll. A wait
timeout only stops the caller from waiting and does not cancel the run.

### Operation matrix

| Operation | Contract | v2 implementation |
| --- | --- | --- |
| `spawn` | Create Agent, process/session, and initial Run | yes, persistent Grok |
| `send` | Queue mailbox message without starting a Run | yes, bounded mailbox |
| `followup` | Start a new Run on the same ACP session | yes, strict session stamp |
| `interrupt` | Cancel one Run | yes, cancel then owned-process escalation |
| `list` | Snapshot Agents, Runs, and provider expectations | yes |
| `wait_run` | Wait for one terminal receipt | yes |
| `events` | Observe a Run's ordered event stream | yes, bounded stream |
| `wait_any` / `wait_all` | Aggregate non-polling waits | yes |

## Run it

Prerequisites:

- Rust 1.96 or newer
- At least one qualified, authenticated provider executable listed in
  [Provider compatibility](PROVIDER_COMPATIBILITY.md)

```bash
cargo build
target/debug/agentmux serve \
  --provider-limit grok=2 \
  --provider-limit codex=4
target/debug/agentmux spawn --provider grok --cwd . \
  "Reply with a one-line summary of this repository"
target/debug/agentmux spawn --profile grok-default --cwd . \
  "Run the qualified Grok profile"
target/debug/agentmux wait RUN_UUID
target/debug/agentmux doctor --json
target/debug/agentmux compatibility status --json
target/debug/agentmux provider verify grok-default --json
target/release/agentmux benchmark --enforce --json
```

Mutation permissions are denied by default. To explicitly allow every provider
permission request:

```bash
target/debug/agentmux spawn --permissions allow-all "Create hello.txt"
```

If a profile uses `version_policy = "experimental"`, mutation-capable
permissions additionally require `--allow-unverified-mutations`. This is a
deliberate double opt-in.

Provider profiles are loaded from `~/.config/agentmux/providers.toml` by
default. Config schema v2 removes user-authored versions, checksums, command
arguments, and self-attested qualification fields. Migrate v1 explicitly with
`agentmux config migrate --check` followed by `--write`; a timestamped backup is
created before an atomic mode-`0600` replacement. Only each Provider Driver's
allowlisted environment variables cross the
supervisor boundary.

`spawn` prints the Agent/Run handles. `wait` prints a JSON `RunReceipt`
containing terminal state, failure taxonomy, output, timings, and cleanup
disposition.

## Invariants

- IDs are UUIDv7 and never derived from names or paths.
- Canonical Run states are exactly `queued`, `running`, `succeeded`, `failed`,
  and `interrupted`.
- Only `queued -> running -> terminal` transitions are valid.
- Startup failures are reported through the normal terminal receipt path.
- stdout belongs exclusively to ACP; provider logs are read from bounded
  stderr capture in the official SDK.
- Successful Runs retain their live provider session; broker shutdown and force
  interrupt wait for the owned process tree to be terminated and reaped.
- No session replay, provider fallback, CLI output scraping, or automatic
  adapter installation is permitted.

## Protocol and provider compatibility

`acpxx` pins `agent-client-protocol = 2.0.0` without unstable features. Despite
the crate version, its default schema remains stable protocol v1; protocol v2
is still behind `unstable_protocol_v2`. See the
[official Rust SDK](https://github.com/agentclientprotocol/rust-sdk).

`serve` and `spawn` never access the network. Only `agentmux compatibility
update` downloads or imports a complete signed Catalog, applies signature,
expiry, schema, Driver, and monotonic-sequence checks, installs an immutable
cache generation, and asks a running IPC v2 daemon to reload it. Existing
Agents keep their original lock, process, and session.

## Scope and roadmap

The authoritative roadmap has Phases 0 through 18. It separates the fake ACP
runtime, host services, owned-process lifecycle, Grok E2E, observation APIs,
continuity, interruption, aggregate waits, daemon/IPC, conformance, each
provider integration, persistence, hardening, and release packaging.

Read:

- [Installation](docs/installation.md)
- [Provider setup and authentication](docs/provider-setup.md)
- [CLI reference](docs/cli-reference.md)
- [Rust API](docs/rust-api.md)
- [Lifecycle semantics](docs/lifecycle-semantics.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Upgrade policy](docs/upgrade-policy.md)
- [Release procedure](docs/release.md)
- [Product contract](PRODUCT_CONTRACT.md)
- [v1 non-goals](NON_GOALS.md)
- [Architecture and phase plan](docs/architecture.ja.md)
- [Current implementation gap](docs/status.ja.md)
- [Provider compatibility](PROVIDER_COMPATIBILITY.md)
- [Provider conformance](CONFORMANCE.md)
- [Performance qualification](PERFORMANCE.md)
- [v2 Definition of Done](V2_DEFINITION_OF_DONE.md)
- [Catalog migration plan](docs/provider-version-catalog-migration-plan.md)
- [ADR-0001](docs/adr/0001-handle-first-acp-runtime.md)

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

The test suite includes a black-box mock ACP process. Authenticated provider
audits are explicit ignored/manual suites because CI does not receive provider
credentials. The release gate requires those suites on the qualified environment.

## License

Apache-2.0.
