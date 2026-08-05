# agentmux v2.0.0 release notes

v2.0.0 replaces provider-version authorization with runtime ACP compatibility checks.

## Breaking changes

- Unknown provider versions are no longer rejected merely because agentmux has not recorded them.
- Exact version/component/digest constraints are now optional user-owned local assertions.
- The old `compatibility status/update` CLI and related Rust/IPC fields are removed.
- `provider inspect PROFILE` reports local executable safety, observations, digest, and assertion
  result without starting an ACP session.
- Provider config remains schema v2 but uses a final, incompatible shape. Run
  `agentmux config migrate --check`, review, then `--write`; a private timestamped backup is kept.
- SQLite metadata migrates transactionally to schema v3 and renames stored lock-shaped data to
  observed provider identity while discarding authorization-only fields.
- IPC v2 is the final pre-publication shape and is not compatible with transient development
  builds of the earlier v2 protocol.

## Runtime contract

Every new Agent validates the canonical launch artifact, observes version/components best-effort,
evaluates optional exact assertions, rechecks file identity, and negotiates stable ACP v1 plus
required capabilities. Permission policy is independent of version. A breaking provider launch,
authentication, ACP, or permission change can still require a `ProviderDriver` update.

agentmux never discovers, installs, or updates providers. Past test results in
`TESTED_PROVIDERS.md` are non-authoritative and have no runtime effect.
