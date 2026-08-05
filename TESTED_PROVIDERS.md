# Tested providers

This information is observational and non-authoritative. It is not a runtime allowlist,
compatibility guarantee, recommended channel, support lifecycle, or maintainer commitment to
track provider releases. Provider versions absent from this table are still attempted through the
standard runtime path.

| Provider | Observed version | Components | Platform | agentmux commit | Tested at | Scope | Result |
|---|---|---|---|---|---|---|---|
| Grok | 0.2.118 | — | macOS arm64 | v2.0.0 source | 2026-08-05 | initialize, 3 turns, stream, cancel recovery, process audit | Passed |
| Cursor | 2026.07.20-8cc9c0b | — | macOS arm64 | v2.0.0 source | 2026-08-05 | initialize, 3 turns, stream, cancel recovery, process audit | Passed |
| Codex | ACP adapter 1.1.9 | Codex 0.145.0 | macOS arm64 | v2.0.0 source | 2026-08-05 | initialize, 3 turns, approval allow/deny, cancel, process audit | Passed |
| Claude | ACP adapter 0.64.2 | Claude Agent SDK 0.3.220 | macOS arm64 | v2.0.0 source | 2026-08-05 | initialize, 3 turns, permission allow/deny, cancel, process audit | Passed |

The table is updated only when useful measurements are made for an agentmux change. It has no
signature, expiry, release-discovery workflow, or effect on execution authorization.
