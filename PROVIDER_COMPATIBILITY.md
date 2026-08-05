# Provider Compatibility

This matrix is deliberately limited to the four v1 providers. `stable` is
awarded only after the common conformance suite is frozen and the exact tested
version passes every mandatory test.

| Provider | ACP path | Tested version | Plan phase | Current status |
| --- | --- | --- | ---: | --- |
| Grok | native `grok agent stdio` | CLI `0.2.118` | 5 and 11 | **stable**; all mandatory suites and pinned capability inventory passed |
| Cursor | native `cursor-agent acp` | CLI `2026.07.20-8cc9c0b` | 12 | **stable**; all mandatory suites and pinned capability inventory passed |
| Codex | pinned `@agentclientprotocol/codex-acp` | adapter `1.1.9`, bundled Codex `0.145.0` | 13 | **stable**; all mandatory suites, real approval audit, and pinned capability inventory passed |
| Claude | pinned `claude-agent-acp` | adapter `0.64.2`, Agent SDK `0.3.220` | 14 | **stable**; all mandatory suites, real permission audit, and pinned capability inventory passed |

Environment verification above was performed on macOS arm64 on 2026-08-05.
The Grok first prompt and missing-authentication path were repeated through the
packaged supervisor's environment allowlist. On the same date, all four pinned
providers passed the enhanced authenticated stream/follow-up/cancel-recovery
suite, real process audit, and capability inventory. Codex and Claude also
passed real permission deny/allow audits with provider metadata preservation.

Codex is launched in the adapter's `read-only` mode with
`approvals_reviewer="user"`. This prevents Codex Guardian auto-review from
bypassing agentmux's default-deny responder. The broker selects permission
options by ACP `kind` and never assumes provider option ordering.

## Stable acceptance

Every provider must pass initialize, authentication-path reporting,
`session/new`, first prompt, streaming order, at least three same-session turns,
cancel/escalation, crash, malformed protocol, no-fallback, process cleanup,
output-boundary, and exact-version checks.

Capabilities such as permission, filesystem, terminal, images, MCP, and session
resume are tested only when advertised. The broker reports unsupported
capabilities rather than emulating them.

## Version policy

- Runtime discovery never installs or updates a provider.
- Every stable profile pins a tested provider or adapter version.
- A different executable may be selected only by explicit configuration.
- `doctor` reports version and capability mismatches without changing the host.

## Primary references

- [Official ACP Rust SDK](https://github.com/agentclientprotocol/rust-sdk)
- [Grok headless and ACP mode](https://docs.x.ai/build/cli/headless-scripting)
- [Cursor ACP documentation](https://cursor.com/docs/cli/acp)
- [Codex ACP adapter](https://github.com/agentclientprotocol/codex-acp)
- [Codex App Server](https://learn.chatgpt.com/docs/app-server)
- [Claude ACP adapter](https://github.com/agentclientprotocol/claude-agent-acp)
