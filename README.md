# acpxx

`acpxx` is a handle-first local broker for coding agents that speak the
[Agent Client Protocol (ACP)](https://agentclientprotocol.com/).

The long-term target is one control API for Codex, Claude, Grok, Cursor, and
Antigravity. The first vertical slice is intentionally narrower: **stable ACP
v1, Grok only, one-shot runs**.

> Status: early v0.1 development. The domain contract is frozen, but only
> `spawn`, `list`, and `wait_run` execute today.

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
  -> exact Grok version probe
  -> grok --no-auto-update agent stdio
  -> ACP initialize (v1)
  -> session/new
  -> session/prompt + streamed session/update collection
  -> process-group cleanup
  -> terminal receipt published to wait_run
```

The implementation uses `tokio::sync::watch`; `wait_run` does not poll. A wait
timeout only stops the caller from waiting and does not cancel the run.

### Operation matrix

| Operation | Contract | v0.1 implementation |
| --- | --- | --- |
| `spawn` | Create Agent, process/session, and initial Run | yes, one-shot Grok |
| `send` | Queue mailbox message without starting a Run | Phase 2 |
| `followup` | Start a new Run on the same ACP session | Phase 2 |
| `interrupt` | Cancel one Run | Phase 3 |
| `list` | Snapshot Agents, Runs, and provider expectations | yes |
| `wait_run` | Wait for one terminal receipt | yes |
| `wait_any` / `wait_all` | Aggregate non-polling waits | Phase 4 |

## Run it

Prerequisites:

- Rust 1.88 or newer
- Grok CLI `0.2.118`, authenticated locally or with `XAI_API_KEY`

```bash
cargo build
cargo run -- run --cwd . "Reply with a one-line summary of this repository"
```

Mutation permissions are denied by default. To explicitly allow every provider
permission request:

```bash
cargo run -- run --permissions allow-all "Create a file named hello.txt"
```

The command prints a JSON `RunReceipt` containing terminal state, failure
taxonomy, output, timings, and cleanup disposition.

## Invariants

- IDs are UUIDv7 and never derived from names or paths.
- Canonical Run states are exactly `queued`, `running`, `succeeded`, `failed`,
  and `interrupted`.
- Only `queued -> running -> terminal` transitions are valid.
- Startup failures are reported through the normal terminal receipt path.
- stdout belongs exclusively to ACP; provider logs are read from bounded
  stderr capture in the official SDK.
- A one-shot receipt is published only after the provider process group has
  been terminated and reaped.
- No session replay, provider fallback, CLI output scraping, or automatic
  adapter installation is permitted.

## Protocol and provider pins

`acpxx` pins `agent-client-protocol = 2.0.0` without unstable features. Despite
the crate version, its default schema remains stable protocol v1; protocol v2
is still behind `unstable_protocol_v2`. See the
[official Rust SDK](https://github.com/agentclientprotocol/rust-sdk).

The first tested provider manifest pins Grok CLI `0.2.118` and launches the
documented native endpoint `grok --no-auto-update agent stdio`. See the
[Grok CLI reference](https://docs.x.ai/build/cli/reference) and
[headless/ACP guide](https://docs.x.ai/build/cli/headless-scripting).

## Scope and roadmap

- Phase 0: domain contract, state machine, receipts, provider manifest — done
- Phase 1: Grok one-shot ACP vertical slice — done
- Phase 2: persistent Agent, mailbox, follow-up, strict continuity
- Phase 3: interrupt, deadlines, parent-death supervisor, Windows Job Object
- Phase 4: aggregate waits and cross-Agent concurrency tests
- Phase 5: Cursor, Codex, then Claude via conformance-tested manifests
- Phase 6: Antigravity experimental evaluation
- Phase 7: doctor, release hardening, soak tests, and compatibility matrix

Read [the architecture plan](docs/architecture.ja.md) and
[ADR-0001](docs/adr/0001-handle-first-acp-runtime.md) for the decisions behind
that sequence.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

The test suite includes a black-box mock ACP process. A real Grok run is a
manual authenticated smoke test and is not required in CI.

## License

Apache-2.0.
