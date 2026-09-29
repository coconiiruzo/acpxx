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

agentmux answers each ACP permission request itself: `deny` selects the provider's one-time reject
option and `allow-all` its one-time allow option, falling back to ACP `cancelled` or a persistent
allow option only when the one-time option is absent.

The Grok driver launches `grok --no-auto-update --permission-mode default agent --no-leader
stdio`. `--permission-mode default` overrides an always-approve or auto default from
`~/.grok/config.toml`, a project `.grok/config.toml`, or Claude-compatible `defaultMode`, so Grok
asks agentmux before mutating tools run; `--no-leader` keeps tool execution in the owned process
even when Grok's shared leader mode is configured. Grok-side permission rules still apply on top of
that mode: `allow` rules in Grok or Claude-compatible settings and "always allow" grants remembered
from earlier Grok sessions for the same project approve matching tools without asking agentmux.
Remove those rules and grants when agentmux's policy must decide every mutation.

Under `allow-all`, agentmux advertises its ACP terminal host and Grok runs shell commands through
it; under `deny` the terminal host is neither advertised nor served. ACP v1 does not specify whether
`terminal/create` `command` may be a whole shell line, and Grok sends one (for example
`/opt/homebrew/bin/bash -lc '...'`) with no `args`. The terminal host therefore runs an
argument-less `command` that contains whitespace and is not an existing file through `/bin/sh -c`;
requests with `args`, and commands that name an executable, are spawned exactly as given.
