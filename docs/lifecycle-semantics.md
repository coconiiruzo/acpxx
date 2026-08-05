# State, continuity, and process semantics

Every Run follows only `queued -> running -> succeeded|failed|interrupted`.
Startup, authentication, prompting, cancellation, and cleanup are stages, not
additional states. Terminal states cannot transition again.

`send` appends to an Agent-owned bounded mailbox. It creates no Run, sends no
ACP prompt, and never steers the active turn. `followup` atomically fixes the
mailbox cutoff and is the only operation that creates a continuation Run.

A continuation is valid only on the same live provider process, ACP transport
generation, adapter instance, profile fingerprint, and provider session ID.
Loss of any element marks continuity lost. agentmux never restarts an adapter,
creates a replacement session, switches provider, or replays a transcript and
calls that continuity.

`interrupt` targets a Run and is idempotent. It first requests ACP cancellation,
then closes stdin, terminates the owned process group, force-kills after grace,
and waits/reaps. Graceful cancellation can retain continuity; force termination
cannot. Execution deadlines use this path. Wait timeouts do not.

The broker owns only handles and process groups it created. It never searches
by PID name, command, cwd, or path. The self-supervisor watchdog collects the
provider and descendants on normal shutdown, cancellation failure, broker
termination, and abrupt broker death. This is containment, not a sandbox and
not a guarantee about provider network behavior.

SQLite stores metadata and redacted terminal receipts, never prompt/output
bodies, reasoning, tool arguments, environment values, credentials, or provider
session secrets. After broker restart, prior active Runs become
`failed(host_restarted)` and all prior Agent continuity is lost.
