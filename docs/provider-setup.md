# Provider setup and authentication

Install and authenticate Codex, Claude, Grok, or Cursor using the provider's own official flow.
Record only absolute executable paths, permission policy, and optional exact local assertions in
`~/.config/agentmux/providers.toml`. The file must be regular and mode `0600`.

```toml
schema_version = 2

[profiles.grok-default]
provider = "grok"
executable = "/absolute/path/to/grok"
permissions = "deny"

[profiles.cursor-default]
provider = "cursor"
executable = "/absolute/path/to/cursor-agent"

[profiles.codex-default]
provider = "codex"
adapter_path = "/absolute/path/to/codex-acp"

[profiles.claude-default]
provider = "claude"
adapter_path = "/absolute/path/to/claude-agent-acp"
```

Assertions are optional user-owned pins:

```toml
[profiles.codex-default.assertions]
version = "1.1.9"
launch_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[profiles.codex-default.assertions.components]
codex = "0.145.0"
```

All assertions are exact. Remove the assertions table to attempt locally installed future versions
through executable safety checks and ACP negotiation. agentmux does not infer compatibility from a
version string and does not discover, install, or update versions.

```bash
chmod 600 ~/.config/agentmux/providers.toml
agentmux provider inspect codex-default --json
agentmux doctor --json
```

Grok and Cursor use existing authenticated CLI state. Codex uses adapter-supported ChatGPT login or
API-key state. Claude uses its existing Claude authentication. Secrets do not belong in the profile;
only driver-allowlisted environment names cross the supervisor. Mutation permissions default to
`deny`; `allow-all` is an explicit policy and behaves identically for familiar and unfamiliar
versions.
