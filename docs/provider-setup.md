# Provider setup and authentication

agentmux supports exactly four provider profiles. It never installs, upgrades,
or silently substitutes a provider executable. Install and authenticate the
provider using that provider's official procedure, then record its absolute
executable path in config schema v2 at `~/.config/agentmux/providers.toml`.

```toml
schema_version = 2

[catalog]
source = "official"

[profiles.grok-default]
provider = "grok"
executable = "/absolute/path/to/grok"
version_policy = "verified"
permissions = "deny"

[profiles.cursor-default]
provider = "cursor"
executable = "/absolute/path/to/cursor-agent"
version_policy = "verified"
permissions = "deny"

[profiles.codex-default]
provider = "codex"
adapter_path = "/absolute/path/to/codex-acp"
version_policy = "verified"
permissions = "deny"

[profiles.claude-default]
provider = "claude"
adapter_path = "/absolute/path/to/claude-agent-acp"
version_policy = "verified"
permissions = "deny"
```

The file must be a regular, current-user-owned file with mode `0600`:

```bash
chmod 600 ~/.config/agentmux/providers.toml
agentmux doctor --json
agentmux provider status --json
```

The qualified identities are maintained in the signed Catalog and generated
[`PROVIDER_COMPATIBILITY.md`](../PROVIDER_COMPATIBILITY.md). Authentication remains provider-owned: Grok and Cursor
use their existing authenticated CLI state; Codex supports its adapter's
ChatGPT login or API-key path; Claude uses its existing Claude authentication.
Secrets are never placed in the profile. Only provider-specific allowlisted
environment variable names cross the supervisor boundary.

Mutation permissions default to `deny`. `--permissions allow-all` is an
explicit unattended policy and should be used only for a trusted prompt and
workspace. Missing auth, an untested version, checksum mismatch, or missing
required capability is reported; agentmux does not repair it automatically.

For an existing schema-v1 file, first run `agentmux config migrate --check`.
Only an exact Catalog identity/artifact match can be migrated automatically;
an unknown local checksum is never promoted to trusted. After resolving any
blocking diagnostics, `agentmux config migrate --write` creates a timestamped
backup and atomically writes the mode-`0600` v2 file.
