# agentmux 2.0 Definition of Done

agentmux 2.0 is complete only while every item below remains true.

- [x] Codex, Claude, Grok, and Cursor are the only Provider IDs.
- [x] Stable ACP v1 remains the only provider wire protocol.
- [x] Provider Drivers own commands, probes, fixed arguments, and capabilities;
  they do not authorize provider versions.
- [x] Exact signed Catalog bytes, schema validation, expiry, and monotonic
  sequence checks protect the active Catalog.
- [x] Bootstrap, immutable generations, redundant same-generation state, and
  offline signed-file import fail closed without automatic rollback.
- [x] Resolver comparison is deny-first and matches identity plus the complete
  artifact set, target, Driver revision, and agentmux requirement.
- [x] Every accepted Agent receives one immutable `ResolvedProviderLock` before
  process launch; follow-ups reuse the same process/session/lock.
- [x] `verified`, `exact`, and explicit `experimental` policies are distinct;
  experimental mutations require a second opt-in.
- [x] Config schema v1 is rejected by normal operation and has explicit dry-run,
  backup, mode-`0600`, atomic schema-v2 migration.
- [x] Unknown v1 checksums are not promoted to trusted Catalog entries.
- [x] Agent snapshots persist the full lock and terminal receipts persist a
  standalone redacted lock summary.
- [x] SQLite schema v2 preserves v1 records with `provider_lock = null`, does
  not infer historical locks, and rejects unknown future schemas.
- [x] IPC v2 exposes Catalog status/reload with explicit version mismatch.
- [x] CLI exposes compatibility status/update, provider status/verify, and
  config migration; verification never starts a provider session.
- [x] `serve` and `spawn` are offline; only explicit compatibility update may
  download Catalog data.
- [x] Candidate discovery cannot publish. Controlled qualification records
  machine-readable evidence and protected publishing emits signed immutable
  sequence assets.
- [x] The public compatibility matrix is generated from the Catalog and checked
  for consistency in tests.
- [x] The legacy 1.0 manifest is historical data outside the runtime source.
- [x] Provider/model time remains separate from broker performance claims.
- [x] macOS arm64 packaging includes the binary, signed bootstrap Catalog,
  schema, checksums, SBOM, licenses, and reproducible-release instructions.

The final release gate is `cargo fmt --check`, clippy with warnings denied, all
automated tests, release packaging tests, and the four credentialed provider
qualification suites on the controlled macOS arm64 environment.
