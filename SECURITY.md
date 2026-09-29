# Security Policy

This project is pre-release. Do not use it as a security boundary for untrusted
providers or prompts.

Please report vulnerabilities privately through
[GitHub Security Advisories](https://github.com/coconiiruzo/acpxx/security/advisories/new).
Do not open a public issue containing credentials, exploit details, or sensitive
prompt/output data.

`agentmux` does not persist prompts, outputs, reasoning, tool arguments, session
stamps, or environment values by default. Metadata profiles must be regular files
with mode `0600`; unknown profile fields are rejected. Permission requests,
filesystem writes, and terminal creation are denied unless an explicit allow
policy is selected. A denied request selects the provider's one-time reject
option. Provider-side allow rules and remembered grants are outside agentmux's
control; see [provider setup](docs/provider-setup.md).

Provider and terminal processes run in owned Unix process groups. The broker uses
the same binary in `__supervise` mode with a watchdog pipe so that normal exit,
SIGTERM, cancellation escalation, and broker death all trigger descendant cleanup.
Adapters that deliberately detach from the owned process group are unsupported.

IPC uses a current-user Unix Domain Socket with mode `0600` and a 1 MiB frame
limit. Filesystem and terminal host services canonicalize paths and reject
symlink escapes from the Agent root. Under an explicit allow policy, an
argument-less terminal command that is a shell line runs through `/bin/sh -c`;
see [provider setup](docs/provider-setup.md). Provider-controlled failure text passes
through credential redaction before persistence.

The v1 release target is macOS arm64. Providers and prompts are not trusted, and
`agentmux` is not a general sandbox or a guarantee about provider network access.
