# Upgrade policy

agentmux follows semantic versioning for its public handle, control,
observation, receipt, and IPC contracts. Patch releases fix compatible defects;
minor releases may add compatible fields or capabilities; incompatible changes
require a major release.

Provider compatibility is independently versioned, signed Catalog data. A new
entry requires exact identity/artifact evidence, authenticated conformance,
process audit, capability inventory, protected approval, and a strictly higher
Catalog sequence. Candidate discovery never publishes. Runtime auto-install,
provider auto-update, rollback, and version-only authorization are prohibited.

Before upgrading:

1. Download and verify checksum plus GitHub attestation.
2. Read the active Catalog status, compatibility matrix, and release notes.
3. Run `agentmux doctor --json` against the new binary and existing profiles.
4. Stop the old broker cleanly, then start the new broker.

An executable upgrade never resumes old provider sessions. Persisted terminal
receipts remain queryable; any prior active Run is reconciled to
`host_restarted`, and prior Agents are continuity-lost. SQLite migrations are
forward-only and transactional. Version 2 migrates schema-v1 records with
`provider_lock = null` and never infers an old lock from the current Catalog.
Downgrading a schema-v2 database to agentmux 1.x is unsupported; back up the
database and provider config before upgrade.
