# PR #4 salvage inventory

Baseline merge `9a9a982`, first parent `daa3626`, 77 changed paths. `KEEP_AS_IS` means the PR #4
version survives unchanged; `FORWARD_PORT` means the central-authorization coupling was removed but
the general hardening was restored; `REWRITE` means the final v2 contract replaced the content;
`DELETE` means the path has no place in the final tree. No item is retained without a reason.

| Path | Class | Reason / regression coverage |
|---|---|---|
| `.github/workflows/compatibility-publish.yml` | DELETE | Per-release publication operation removed; static release-manifest test |
| `.github/workflows/provider-candidate-discovery.yml` | DELETE | Version discovery is not a product responsibility |
| `.github/workflows/provider-qualification.yml` | DELETE | Conformance remains manual/runtime-oriented, not a publication pipeline |
| `CONFORMANCE.md` | REWRITE | Driver/runtime conformance without version promotion |
| `Cargo.lock` | REWRITE | v2 package metadata; dedicated crypto/network dependencies absent |
| `Cargo.toml` | REWRITE | v2 version and runtime-only dependency set |
| `NON_GOALS.md` | REWRITE | Provider tracking/registry/update explicitly excluded |
| `PRODUCT_CONTRACT.md` | REWRITE | Runtime negotiation and local assertions are normative |
| `PROVIDER_COMPATIBILITY.md` | DELETE | Replaced by non-authoritative `TESTED_PROVIDERS.md` |
| `README.md` | REWRITE | v2 behavior and migration-facing overview |
| `V2_DEFINITION_OF_DONE.md` | REWRITE | Final runtime-compatibility release gates |
| `compatibility/bootstrap/catalog-v1.json` | DELETE | No bootstrap authorization data |
| `compatibility/bootstrap/catalog-v1.sig` | DELETE | No runtime data signatures |
| `compatibility/evidence/README.md` | DELETE | Qualification evidence operation removed |
| `compatibility/evidence/bootstrap-2026-08-05.json` | DELETE | Obsolete evidence asset |
| `compatibility/legacy/README.md` | DELETE | Legacy parser is code/test-fixture scoped only |
| `compatibility/legacy/agentmux-1.0.0.json` | DELETE | Old data is not shipped in main |
| `compatibility/schema/catalog-v1.schema.json` | DELETE | Obsolete data schema |
| `docs/adr/0005-provider-compatibility-catalog.md` | REWRITE | Historical decision retained and marked superseded |
| `docs/architecture.ja.md` | REWRITE | Observation/assertion/ACP flow and identity architecture |
| `docs/cli-reference.md` | REWRITE | `provider inspect` and `config migrate`; removed routes absent |
| `docs/installation.md` | REWRITE | Install works without external authorization data |
| `docs/provider-setup.md` | REWRITE | Final config v2 and optional local assertions |
| `docs/provider-version-catalog-migration-plan.md` | DELETE | Superseded implementation plan; ADR history retained |
| `docs/release.md` | REWRITE | Runtime data-free reproducible package |
| `docs/rust-api.md` | REWRITE | `ProviderAssertions` and execution identity |
| `docs/status.ja.md` | REWRITE | v2 implementation status |
| `docs/troubleshooting.md` | REWRITE | Assertion/artifact/ACP failures separated |
| `docs/upgrade-policy.md` | REWRITE | Provider releases do not require data updates |
| `packaging/homebrew/agentmux.rb.in` | REWRITE | Checksum-bound binary test retained; obsolete status command removed |
| `scripts/package-release.sh` | FORWARD_PORT | Reproducibility retained; obsolete asset staging removed |
| `scripts/record-provider-qualification.py` | DELETE | No qualification publication operation |
| `scripts/render-provider-compatibility.py` | DELETE | No generated authorization document |
| `scripts/sign-provider-catalog.sh` | DELETE | No runtime metadata signing |
| `src/acp/client.rs` | FORWARD_PORT | Error/stderr/output hardening retained; runtime negotiation classification tested |
| `src/acp/mod.rs` | FORWARD_PORT | Observation API exported without obsolete module dependency |
| `src/acp/session.rs` | FORWARD_PORT | Bounded probes, safety, TOCTOU, ACP identity/capability gates; runtime suite |
| `src/api.rs` | REWRITE | Constructors and snapshots have no central state; public JSON contract test |
| `src/compatibility/bootstrap.rs` | DELETE | Bootstrap state removed |
| `src/compatibility/catalog.rs` | DELETE | Catalog parser/validator removed |
| `src/compatibility/mod.rs` | DELETE | Generic identity moved to `provider_identity.rs` |
| `src/compatibility/model.rs` | DELETE | Authorization models removed; audit models rebuilt |
| `src/compatibility/resolver.rs` | DELETE | Runtime does not resolve against a version list |
| `src/compatibility/signature.rs` | DELETE | Signature verifier removed |
| `src/compatibility/store.rs` | DELETE | Update/cache store removed |
| `src/config.rs` | REWRITE | Final schema v2 plus offline legacy migration; config tests |
| `src/doctor.rs` | REWRITE | Local safety/probe/assertion diagnostics only |
| `src/ipc.rs` | REWRITE | Final IPC v2 without update/status routes |
| `src/lib.rs` | REWRITE | Exports provider identity, not central models |
| `src/main.rs` | REWRITE | Inspect/migrate CLI and version-free spawn path |
| `src/model.rs` | REWRITE | Snapshots carry `provider_identity` |
| `src/process/owner.rs` | FORWARD_PORT | Owned process/tree hardening; process and fault suites |
| `src/providers/claude.rs` | REWRITE | Behavior/probes only; no tested version constant |
| `src/providers/codex.rs` | REWRITE | Behavior/probes and permission safeguards only |
| `src/providers/cursor.rs` | REWRITE | Native ACP driver with best-effort exact-output probe |
| `src/providers/grok.rs` | REWRITE | Native ACP driver with best-effort semver observation |
| `src/providers/mod.rs` | REWRITE | `ProviderDriver`, local assertions, closed four-provider API |
| `src/receipt.rs` | REWRITE | New failure taxonomy and `provider_identity` output |
| `src/runtime/agent_actor.rs` | FORWARD_PORT | Immutable identity/continuity and race serialization; continuity suite |
| `src/runtime/registry.rs` | FORWARD_PORT | Identity snapshots and metadata restore; persistence suite |
| `src/storage.rs` | REWRITE | Transactional SQLite v3 and legacy JSON transformation |
| `tests/catalog_release.rs` | DELETE | Replaced by static central-infrastructure absence test |
| `tests/catalog_runtime.rs` | DELETE | Replaced by provider runtime compatibility suite |
| `tests/compatibility_catalog.rs` | DELETE | Sequence/signature/expiry behavior intentionally removed |
| `tests/config.rs` | REWRITE | Final v2, v1 migration, transient-v2 migration, backup/security |
| `tests/continuity.rs` | FORWARD_PORT | Same-process/session invariants retained without authorization lock |
| `tests/contract.rs` | REWRITE | Final public JSON/CLI/provider-set contract |
| `tests/ipc.rs` | FORWARD_PORT | IPC isolation/version/bounds and persistent-Agent behavior |
| `tests/minimal_e2e.rs` | FORWARD_PORT | ACP lifecycle, output bounds, failure projection |
| `tests/persistence.rs` | REWRITE | SQLite v0/v1/v2-to-v3, rollback, privacy, restart |
| `tests/provider_capability_audit.rs` | REWRITE | Observational runtime capability inventory |
| `tests/provider_conformance.rs` | REWRITE | Authenticated driver smoke without version authorization |
| `tests/provider_fault_conformance.rs` | FORWARD_PORT | Cross-driver faults, permissions, cleanup, no fallback |
| `tests/provider_permission_audit.rs` | FORWARD_PORT | Version-independent real mutation policy checks |
| `tests/release_manifest.rs` | REWRITE | Tested-document disclaimer and infrastructure absence |
| `tests/startup_failure.rs` | FORWARD_PORT | Startup failure/cleanup classification |
| `tests/support/mod.rs` | FORWARD_PORT | Isolated executable/package fixtures for runtime observations |

The 77 classifications are enforced collectively by `cargo test --all-targets`, the static absence
commands in the migration plan, authenticated/manual provider suites, reproducible packaging, and
the new runtime/persistence/config contract tests.
