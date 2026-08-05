# ADR-0004: Metadata-only persistence and restart reconciliation

- Status: accepted for v1
- Date: 2026-08-05

## Context

Terminal receipts must remain queryable after broker restart, while session
continuity cannot be proven after the live provider process is lost. Persisting
full transcripts also expands the privacy and secret-handling surface.

## Decision

Use SQLite for Agent/Run identifiers and metadata, state, timestamps, failure
and stop reason, terminal receipt, completion sequence, and tested executable
versions. Do not persist prompts, assistant output, reasoning, complete tool
arguments, credentials, session secrets, or environment values by default.

At broker startup, persisted queued or running Runs become
`failed(host_restarted)`. Persisted Agents remain listable but their continuity
becomes `lost(host_restarted)`. Startup never launches providers, resumes a
session, creates a replacement session, or replays a transcript.

## Consequences

- Terminal receipts survive broker restart without claiming Agent recovery.
- Recovery is deterministic even after an unclean exit.
- Full audit logs require a separately designed opt-in feature.
- Database corruption must fail closed and must not start a provider.
