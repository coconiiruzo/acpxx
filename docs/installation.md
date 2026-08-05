# Installation

agentmux v2 supports macOS arm64. Provider CLIs and ACP adapters are separate
executables and are not downloaded or updated by agentmux.

## Release archive

Download the `agentmux-VERSION-aarch64-apple-darwin.tar.gz`, `SHA256SUMS`, and
the corresponding SPDX file from the GitHub release. Verify both the checksum
and its GitHub/Sigstore build-provenance attestation before installation:

```bash
shasum -a 256 -c SHA256SUMS
gh attestation verify agentmux-VERSION-aarch64-apple-darwin.tar.gz \
  -R coconiiruzo/acpxx
tar -xzf agentmux-VERSION-aarch64-apple-darwin.tar.gz
install -m 0755 agentmux-VERSION-aarch64-apple-darwin/agentmux /usr/local/bin/agentmux
agentmux --version
```

The release also contains `agentmux.rb`, a checksum-bound Homebrew formula:

```bash
brew install --formula ./agentmux.rb
```

## Source build

Rust 1.96 or newer, Python 3.11 or newer, and the macOS command-line tools are
required. A locked source build is:

```bash
cargo build --locked --release
install -m 0755 target/release/agentmux /usr/local/bin/agentmux
```

After installing, inspect the embedded signed bootstrap with `agentmux
compatibility status`, configure a schema-v2 provider profile, start `agentmux
serve`, and run `agentmux doctor --json`. The mutable Catalog cache is created
only by explicit update under `$XDG_DATA_HOME/agentmux/compatibility`, or under
`~/Library/Application Support/agentmux/compatibility` when XDG is unset. See
[Provider setup](provider-setup.md).

Upgrade from v1 begins with `agentmux config migrate --check`; normal v2
commands intentionally refuse to interpret a v1 profile file.
