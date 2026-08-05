# Release procedure

v1 releases target `aarch64-apple-darwin`. The checked-in packaging path uses a
locked dependency graph and deterministic archive metadata.

## Local reproduction

```bash
rustup target add aarch64-apple-darwin
SOURCE_DATE_EPOCH=0 scripts/package-release.sh
shasum -a 256 dist/agentmux-*-aarch64-apple-darwin.tar.gz
```

Running the script twice from the same source and Rust toolchain must yield the
same archive checksum. It emits the binary archive, `SHA256SUMS`, SPDX 2.3 SBOM,
third-party license inventory, and checksum-bound Homebrew formula under
`dist/`.

## Maintainer release

1. Confirm the Cargo version equals the compatibility-manifest filename/version.
2. Run fmt, clippy, all tests, ignored 10,000-Run and 1,000-process soaks, the
   four authenticated provider suites, and the release benchmark.
3. Commit the release state and create an annotated `vVERSION` tag.
4. Push the tag. `.github/workflows/release.yml` runs on a GitHub-hosted macOS
   arm64 runner, packages the artifact, creates Sigstore/GitHub provenance and
   SBOM attestations, and publishes all files to the GitHub release.
5. Download the published archive and verify it independently:

```bash
shasum -a 256 -c SHA256SUMS
gh attestation verify agentmux-VERSION-aarch64-apple-darwin.tar.gz \
  -R coconiiruzo/acpxx
```

The workflow rejects a tag that does not match `Cargo.toml`. No signing secret,
provider credential, or automatic provider installer is stored in the release
job.
