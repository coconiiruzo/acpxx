# Provider Compatibility

> Generated from `compatibility/bootstrap/catalog-v1.json`; do not edit the table by hand.

Catalog `agentmux-official` sequence `1` was generated at `2026-08-05T00:00:00Z` and expires at `2027-08-05T00:00:00Z`.

| Provider | Identity | Components | Driver | Target | State | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Claude | `0.64.2` | claude_agent_sdk 0.3.220 | `claude-agent-acp` r1 | `aarch64-apple-darwin` | **verified** | `sha256:57c4d05d7b9074e1d09c71a42bb46cebf30ef53a984bd394cdc607697a90bfb1` |
| Codex | `1.1.9` | codex 0.145.0 | `codex-acp` r1 | `aarch64-apple-darwin` | **verified** | `sha256:57c4d05d7b9074e1d09c71a42bb46cebf30ef53a984bd394cdc607697a90bfb1` |
| Cursor | `2026.07.20-8cc9c0b` | — | `cursor-native` r1 | `aarch64-apple-darwin` | **verified** | `sha256:57c4d05d7b9074e1d09c71a42bb46cebf30ef53a984bd394cdc607697a90bfb1` |
| Grok | `0.2.118` | — | `grok-native` r1 | `aarch64-apple-darwin` | **verified** | `sha256:57c4d05d7b9074e1d09c71a42bb46cebf30ef53a984bd394cdc607697a90bfb1` |

Runtime authorization requires an exact identity and artifact-set match against the signed Catalog.
A version string alone is never sufficient. Candidate discovery does not publish or authorize a version;
only controlled qualification plus an approved, higher-sequence signed Catalog update can do so.

Use `agentmux compatibility status` and `agentmux provider verify PROFILE` to inspect the active
Catalog and an installed artifact without starting a provider session.
