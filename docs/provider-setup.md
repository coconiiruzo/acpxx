# Provider setup and authentication

agentmux supports exactly four provider profiles. It never installs, upgrades,
or silently substitutes a provider executable. Install and authenticate the
tested provider version using that provider's official procedure, then record
its absolute executable path in `~/.config/agentmux/providers.toml`.

```toml
[profiles.grok-default]
provider = "grok"
executable = "/absolute/path/to/grok"
disable_auto_update = true

[profiles.cursor-default]
provider = "cursor"
executable = "/absolute/path/to/cursor-agent"

[profiles.codex-default]
provider = "codex"
adapter_path = "/absolute/path/to/codex-acp"
approval = "ask"

[profiles.claude-default]
provider = "claude"
adapter_path = "/absolute/path/to/claude-agent-acp"
```

The file must be a regular, current-user-owned file with mode `0600`:

```bash
chmod 600 ~/.config/agentmux/providers.toml
agentmux doctor --json
```

The tested versions are maintained in
[`PROVIDER_COMPATIBILITY.md`](../PROVIDER_COMPATIBILITY.md) and the release's
versioned JSON manifest. Authentication remains provider-owned: Grok and Cursor
use their existing authenticated CLI state; Codex supports its adapter's
ChatGPT login or API-key path; Claude uses its existing Claude authentication.
Secrets are never placed in the profile. Only provider-specific allowlisted
environment variable names cross the supervisor boundary.

Mutation permissions default to `deny`. `--permissions allow-all` is an
explicit unattended policy and should be used only for a trusted prompt and
workspace. Missing auth, an untested version, checksum mismatch, or missing
required capability is reported; agentmux does not repair it automatically.
