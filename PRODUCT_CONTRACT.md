# agentmux Product Contract

Status: v1 contract candidate
Last updated: 2026-08-05

`agentmux` is a handle-first local broker for exactly four coding-agent
providers: Codex, Claude, Grok, and Cursor. This document defines the v1 product
boundary. The current repository and crate retain the working name `acpxx`
until a dedicated rename change.

## Product shape

The finished product is one Rust binary with these roles:

```text
CLI client -> Unix Domain Socket -> local broker
                                  -> Agent / Run runtime
                                  -> ACP client
                                  -> process supervisor
                                  -> SQLite receipt store
```

Provider CLIs and existing ACP adapters remain external executables. Stable ACP
protocol v1 over stdio is the only provider transport in v1. The first required
release target is macOS arm64; the local API does not expose TCP or HTTP.

The public provider set is closed for v1:

```rust
pub enum ProviderId {
    Codex,
    Claude,
    Grok,
    Cursor,
}
```

There is no public custom-provider escape hatch. Scriptable fake agents are
internal conformance fixtures and do not appear in the public provider model.

## Identity

`AgentId`, `RunId`, and `MessageId` are opaque UUIDv7 values. Provider names,
filesystem paths, working directories, process IDs, session IDs, display names,
and tree paths are metadata and must never resolve an operation target.

```rust
pub struct AgentHandle {
    pub agent_id: AgentId,
}

pub struct RunHandle {
    pub agent_id: AgentId,
    pub run_id: RunId,
}
```

One `AgentId` represents one logical worker, one live provider-process
generation, one ACP connection, and one ACP session. An Agent may have at most
one active Run. Different Agents may run concurrently.

## Public API

The five control operations are:

```rust
async fn spawn(request: SpawnRequest) -> Result<SpawnReceipt>;
async fn send(agent: AgentHandle, message: AgentMessage) -> Result<MessageReceipt>;
async fn followup(
    agent: AgentHandle,
    after: RunId,
    task: FollowupTask,
) -> Result<RunHandle>;
async fn interrupt(run: RunHandle) -> Result<InterruptReceipt>;
async fn list(query: ListQuery) -> Result<ListSnapshot>;
```

The read-only observation API is:

```rust
async fn wait_run(run: RunHandle, options: WaitOptions) -> Result<RunReceipt>;
async fn wait_any(
    runs: NonEmpty<RunHandle>,
    options: WaitOptions,
) -> Result<RunReceipt>;
async fn wait_all(
    runs: NonEmpty<RunHandle>,
    options: WaitOptions,
) -> Result<Vec<RunReceipt>>;
fn events(run: RunHandle) -> impl Stream<Item = RunEvent>;
```

`events` is an observation surface, not a sixth control operation.

### `send`

`send` appends a sequenced entry to the runtime-owned Agent mailbox. It creates
no Run, sends no ACP prompt, starts or resumes nothing, and never steers an
active Run. A later accepted `followup` captures a mailbox sequence cutoff and
drains only entries at or before that cutoff.

### `followup`

`followup(agent, after, task)` is admitted only when:

- the Agent and `after` Run exist;
- `after` belongs to the Agent, is terminal, and is its latest Run;
- no Run for that Agent is active or queued;
- the provider process is alive and continuity is available; and
- provider session ID, transport generation, adapter instance, and provider
  profile fingerprint still match the previous Run's session stamp.

After admission it creates one Run and prompts the same ACP session. A known
continuity loss before admission is an API error. A loss after admission uses
`queued -> running -> failed` with `failure.code = continuity_lost`.

Replacing the process or session, changing providers, or replaying a transcript
does not preserve continuity. v1 does not resume sessions after process restart,
even where ACP advertises a resume capability.

### `interrupt`

`interrupt` targets one Run. A graceful ACP cancel may preserve continuity. If
process escalation is required, the Run is interrupted and Agent continuity is
lost. A Run deadline uses this same path; a wait timeout never interrupts a Run.

## State model

The only canonical Run states are:

```text
queued -> running -> succeeded
                  -> failed
                  -> interrupted
```

The only legal transitions are `Queued -> Running` and `Running` to one of the
three terminal states. Startup, authentication, cancellation, cleanup, timeout,
and continuity details belong to stage, stop reason, failure, and flags—not new
states. Agent availability is derived from orthogonal snapshot fields rather
than a second status state machine.

## Error boundary

Admission errors occur before a Run is created:

```text
invalid_provider  invalid_cwd  agent_not_found  run_not_found
stale_parent      agent_busy   continuity_already_lost
```

Failures after admission terminate the accepted Run:

```text
provider_spawn_failed  acp_initialize_failed  authentication_failed
session_create_failed  prompt_failed           provider_crashed
protocol_corruption    continuity_lost         deadline_exceeded
cleanup_incomplete     host_shutdown           host_restarted
```

## Persistence and privacy

SQLite stores Agent/Run metadata, terminal receipts, completion sequence, and
tested provider/adapter versions. Prompt bodies, assistant output, reasoning,
full tool arguments, secrets, session secrets, and environment values are not
persisted by default.

After broker restart, previously queued or running Runs become failed with
`host_restarted`, and retained Agents become continuity-lost. Restart never
launches a provider or attempts session recovery automatically.
