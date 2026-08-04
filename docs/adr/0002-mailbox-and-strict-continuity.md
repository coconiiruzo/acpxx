# ADR-0002: Runtime mailbox and strict continuity

- Status: accepted, implementation deferred to Phase 2
- Date: 2026-08-05

## Decision

`send` appends to a runtime-owned per-Agent mailbox and never calls
`session/prompt`, creates a Run, starts the Agent, or steers an active Run.
`followup(previous_run, task)` is the only operation that drains accepted mailbox
messages into a new prompt and starts a new Run.

Follow-up is allowed only when the parent is terminal and is the latest Run, no
other Run is active or queued, and the provider session stamp is unchanged.

If process, transport generation, adapter instance, protocol version, or
provider session identity changes, the Run fails with `continuity_lost`.
Creating a replacement session or replaying a transcript is not continuity.

## Consequences

- Concurrent callers cannot silently fork one Agent's logical history.
- Mailbox acceptance order is owned by one actor.
- Provider crash is terminal for that logical Agent in live-process mode.
- Durable recovery requires a future adapter capability that verifies restoration
  of the same provider session ID.
