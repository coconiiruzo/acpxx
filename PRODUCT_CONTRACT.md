# agentmux v2 Product Contract

Status: frozen for v2.0.0
Last updated: 2026-08-05

agentmux is a local, handle-first broker for exactly Codex, Claude, Grok, and Cursor. It is one
Rust binary acting as CLI client, Unix Domain Socket broker, Agent/Run runtime, ACP client, process
supervisor, and metadata-only SQLite store. Provider executables and adapters remain external.
Stable ACP v1 over stdio is the only provider transport; macOS arm64 is the release target.

## Identity and control

`AgentId`, `RunId`, and `MessageId` are opaque UUIDv7 values. Names, cwd, filesystem paths,
provider/process/session IDs, and display paths MUST NOT resolve an operation target. One Agent is
one logical worker, live process generation, ACP transport, and ACP session, with at most one active
Run. Different Agents may execute concurrently.

The five control methods are `spawn`, `send`, `followup`, `interrupt`, and `list`. Read-only
observation is `events`, `wait_run`, `wait_any`, and `wait_all`.

- `send` MUST queue a sequenced mailbox entry, create no Run, issue no ACP prompt, and never steer
  an active Run.
- `followup(agent, after, task)` MUST require the latest terminal parent, an idle live Agent, and
  unchanged session stamp and provider execution identity. It MUST prompt the same ACP session.
- a process/transport/session replacement or transcript replay MUST NOT count as continuity.
- `interrupt` MUST target one Run. A deadline MUST use the same cancellation/escalation path.
- a wait timeout or IPC client disconnect MUST NOT cancel a Run.

The only states are `queued`, `running`, `succeeded`, `failed`, and `interrupted`; the only legal
transitions are `queued -> running` and `running -> terminal`. Startup, authentication,
cancellation, cleanup, and continuity details belong to stages, failures, reasons, and flags.

## Provider runtime compatibility

agentmux MUST:

- attempt an unknown or non-semver provider version through the normal path;
- select one of the four built-in `ProviderDriver`s and keep driver-owned args/env/auth behavior;
- canonicalize and require a regular, executable, current-user/root-owned, non-group/world-writable
  launch artifact;
- observe launch SHA-256 and file identity and detect replacement before spawn;
- treat version and component probes as bounded, best-effort observations;
- evaluate configured local version/component/digest assertions by exact match;
- negotiate stable ACP v1 at runtime and check capabilities needed by an operation;
- apply the same permission policy regardless of observed version; and
- put the observed `ProviderExecutionIdentity` in Agent/Run snapshots and receipts.

agentmux MUST NOT:

- use central version data as runtime authorization;
- infer compatibility, safety, or trust from a version string;
- require maintainers to publish data for each provider release;
- impose an extra mutation denial because a version is unfamiliar;
- automatically install, update, replace, or fall back between providers; or
- download provider metadata during `serve`, `spawn`, or `provider inspect`.

An optional `ProviderAssertions` value contains only exact `version`, exact component values, and
an exact launch SHA-256. Empty assertions are the default. Probe failure is not terminal without a
corresponding assertion. Safety failure, assertion mismatch/unverifiability, artifact replacement,
ACP negotiation failure, or required-capability absence is terminal.

## Continuity and audit identity

The continuity fingerprint includes provider, driver ID/revision, canonical executable path,
launch digest, driver args/fixed environment, and permission-affecting profile settings. It does
not express authorization. The live Agent keeps its process/session identity even if the file on
disk changes; only a new Agent observes the changed artifact.

Receipts distinguish admission errors from accepted-Run failures. Relevant failure codes include
`provider_spawn_failed`, `provider_assertion_failed`, `provider_artifact_changed`,
`acp_initialize_failed`, `authentication_failed`, `session_create_failed`, `prompt_failed`,
`protocol_corruption`, `provider_crashed`, `continuity_lost`, `deadline_exceeded`,
`cleanup_incomplete`, `host_shutdown`, and `host_restarted`.

## Configuration, IPC, and persistence

Final provider config schema is v2, IPC is v2, and SQLite is v3. Config MUST be a bounded regular
0600 file, use absolute executable paths, reject unknown fields, and migrate with a private backup
and atomic replacement. Legacy exact pins MUST become local assertions rather than disappear.

SQLite stores Agent/Run metadata, terminal receipts, completion order, and provider identity. It
MUST NOT persist prompts, assistant output, reasoning, full tool arguments, secrets, provider
session secrets, or environment values by default. Restart MUST reconcile active Runs to
`host_restarted`, mark prior Agents continuity-lost, and never relaunch a provider automatically.

## Process ownership and scope

agentmux owns only process handles/groups it created. Cancellation escalates from ACP cancel to
stdin close and owned TERM/KILL/reap. It MUST NOT search or kill by command name, cwd, or guessed
PID. The v2 non-goals in [NON_GOALS.md](NON_GOALS.md) are normative.
