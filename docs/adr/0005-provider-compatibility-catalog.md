# ADR 0005: Signed provider Compatibility Catalog

Status: accepted for agentmux 2.0.0

## Context

agentmux 1.x compiles one tested version per provider into the binary and asks
profiles to repeat version, checksum, and qualification evidence. That is safe
but couples every compatible provider release to an agentmux binary release
and treats user-authored evidence as authority.

## Decision

Keep exactly four built-in `ProviderDriver`s. Drivers own launch behavior,
identity and artifact observation, environment, authentication, permissions,
ACP transport, and process supervision. A separately signed Compatibility
Catalog owns the set of accepted exact provider identities and artifact
digests for a target and Driver revision.

Catalog bytes are signed directly with detached Ed25519 signatures. The
binary embeds a public-key ring and a signed bootstrap Catalog. Explicit
updates install immutable Catalog generations after signature, schema,
expiry, and monotonically increasing sequence validation. `spawn` performs no
network access.

At Agent initialization, the runtime resolves the selected profile, observed
identity, artifact digests, target, Driver revision, and an immutable Catalog
snapshot into a `ResolvedProviderLock`. Existing Agents and followups retain
that lock, process, transport, and session even when Catalog data changes.

The policies are:

- `verified`: exact match in an active verified/deprecated Catalog entry;
- `exact`: exact match to a named Catalog entry; and
- `experimental`: recorded but unverified identity, with known-blocked denial
  and a separate mutation opt-in.

Catalog entries cannot carry commands, arguments, paths, environment values,
or permission policy. Provider binaries and adapters are never downloaded or
updated by agentmux.

## Trust and rollback

The offline runtime trust root is the embedded Ed25519 keyring. Catalog
sequence is monotonic per Catalog ID. A bad entry is corrected by publishing a
higher sequence that deprecates or blocks it; an older Catalog is never
reactivated as rollback. Signing private keys are not stored in this
repository or ordinary CI jobs.

## Consequences

Provider compatibility can advance without changing the agentmux binary when
the built-in Driver contract is unchanged. Driver behavior changes still
require a binary release and a new Driver revision. Config, receipt,
persistence, Rust API, and IPC contracts change incompatibly, so this is a
2.0.0 migration.
