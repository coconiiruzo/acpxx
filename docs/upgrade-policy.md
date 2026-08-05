# Upgrade policy

agentmux follows semantic versioning for Rust API, receipt, config, SQLite, and IPC contracts.
Provider releases are independent: new versions are attempted without an agentmux data update.
Only a breaking launch command, authentication, ACP, permission, or driver-interface change requires
a `ProviderDriver` code update and driver-revision increment.

agentmux never discovers, installs, updates, recommends, blocks, or falls back between provider
versions. `TESTED_PROVIDERS.md` is historical observation only.

## Upgrade to v2.0.0

1. Verify the release checksum and GitHub attestation.
2. Stop the old broker cleanly and back up config/database files.
3. Run `agentmux config migrate --check --json`.
4. Review preserved local assertions. Delete them only if you want future local versions to float.
5. Run `agentmux config migrate --write`, then `provider inspect` and `doctor`.
6. Start the v2 broker. SQLite v1/v2 migrates transactionally to v3.

Config write creates `providers.toml.pre-runtime-compat-<time>.bak` with mode 0600. An exact legacy
entry that cannot be converted blocks migration rather than silently dropping the constraint.
Migration performs no network access.

Old active Runs become `host_restarted`; prior Agents become continuity-lost. The broker never
relaunches a process or restores a session automatically. A database with a newer unknown schema is
refused without rewriting it.
