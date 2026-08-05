# Installation

agentmux v2 targets macOS arm64. Provider CLIs and ACP adapters are separate executables and are
never downloaded or updated by agentmux.

## Release archive

```bash
shasum -a 256 -c SHA256SUMS
gh attestation verify agentmux-2.0.0-aarch64-apple-darwin.tar.gz \
  -R coconiiruzo/acpxx
tar -xzf agentmux-2.0.0-aarch64-apple-darwin.tar.gz
install -m 0755 agentmux-2.0.0-aarch64-apple-darwin/agentmux /usr/local/bin/agentmux
agentmux --version
```

The checksum-bound Homebrew formula may instead be installed with
`brew install --formula ./agentmux.rb`.

## Source

Rust 1.96+, Python 3.11+, and macOS command-line tools are required.

```bash
cargo build --locked --release
install -m 0755 target/release/agentmux /usr/local/bin/agentmux
```

Install and authenticate a provider independently, create a final-schema profile as described in
[provider setup](provider-setup.md), then run:

```bash
agentmux config migrate --check
agentmux provider inspect PROFILE --json
agentmux doctor --json
agentmux serve
```

No network access by agentmux is needed to validate provider version data.
