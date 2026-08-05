# Upgrade policy

agentmux follows semantic versioning for its public handle, control,
observation, receipt, and IPC contracts. Patch releases fix compatible defects;
minor releases may add compatible fields or capabilities; incompatible changes
require a major release.

Provider and adapter pins are release data, not floating ranges. Updating one
requires a separate change, exact version/checksum, authenticated conformance,
process audit, capability inventory, compatibility-manifest update, and core
regression run. Runtime auto-install and auto-update are prohibited.

Before upgrading:

1. Download and verify checksum plus GitHub attestation.
2. Read the versioned compatibility manifest and release notes.
3. Run `agentmux doctor --json` against the new binary and existing profiles.
4. Stop the old broker cleanly, then start the new broker.

An executable upgrade never resumes old provider sessions. Persisted terminal
receipts remain queryable; any prior active Run is reconciled to
`host_restarted`, and prior Agents are continuity-lost. SQLite migrations are
forward-only and transactional. A binary refuses a database with a newer
unknown schema instead of rewriting it.
