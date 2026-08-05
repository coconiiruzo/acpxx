# ADR 0005: Signed provider Compatibility Catalog

Status: superseded by ADR-0006 before v2.0.0 publication

## Historical context

PR #4 replaced compile-time exact provider pins with a separately signed list of accepted exact
provider identities. The proposal kept four built-in drivers, embedded a public-key ring and signed
bootstrap data, supported explicit immutable updates, and resolved a profile plus observed artifact
into an authorization lock. It also separated useful launch observation and process hardening from
provider-specific behavior.

## Historical decision

The list would have owned accepted versions, artifact digests, sequence, expiry, deprecation, and
blocked state while drivers retained command, args, environment, authentication, ACP, and process
behavior. Unfamiliar versions required a special policy and a separate mutation opt-in. Provider
binaries were never downloaded.

## Supersession

This design was reverted before release because it still required maintainers to discover, test,
sign, and publish provider releases even when the driver contract had not changed. The final v2
design retains local artifact safety, observations, TOCTOU protection, identity auditing, and strict
continuity, but separates observations from authorization. Unknown versions use ACP runtime
negotiation, and user-owned exact constraints use local assertions. See ADR-0006.
