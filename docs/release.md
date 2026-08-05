# v2 release procedure

Releases target `aarch64-apple-darwin`, use the locked dependency graph, and produce deterministic
archive metadata. Runtime provider-version data is not packaged.

## Gates

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo test --test lifecycle_soak fake_lifecycle_ten_thousand_runs -- --ignored
cargo test --test supervisor one_thousand_short_lived_provider_processes_are_reaped -- --ignored --exact
cargo test --test provider_conformance -- --ignored --test-threads=1
cargo test --test provider_capability_audit -- --ignored --test-threads=1
cargo test --test provider_permission_audit -- --ignored --test-threads=1
cargo test --test provider_process_audit -- --ignored --test-threads=1
cargo run --release --bin agentmux -- benchmark --enforce --json
```

Run the static absence checks from the migration plan and verify config v1/transient-v2 plus SQLite
v1/v2 fixtures. Record only real measurements actually obtained in `TESTED_PROVIDERS.md`.

## Reproducible package

```bash
rustup target add aarch64-apple-darwin
SOURCE_DATE_EPOCH=0 scripts/package-release.sh
shasum -a 256 dist/agentmux-2.0.0-aarch64-apple-darwin.tar.gz
```

Run twice from the same source/toolchain and require identical SHA-256. The archive contains the
binary, README, LICENSE, SECURITY, `TESTED_PROVIDERS.md`, SBOM, and third-party license inventory.
`SHA256SUMS` and a checksum-bound Homebrew formula are emitted beside it.

## Publish

1. Confirm `Cargo.toml` is `2.0.0`, all gates pass, and release notes cover the removal of central
   version authorization, optional local assertions, `provider inspect`, config migration, SQLite
   v3, and IPC v2 incompatibility.
2. Commit the release state and create an annotated `v2.0.0` tag.
3. Push the tag. `.github/workflows/release.yml` tests, packages, attests provenance/SBOM, and
   creates the GitHub release on macOS arm64.
4. Download and independently verify checksum and attestation.

No provider credential, update service, signing secret for runtime metadata, or automatic provider
installer belongs in the release job.
