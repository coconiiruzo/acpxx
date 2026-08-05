# ADR 0006: Runtime-negotiated provider compatibility

Status: accepted for v2.0.0

## Context

A centrally maintained exact-version authorization mechanism makes every compatible provider
release an agentmux operations burden. Version strings also cannot prove that ACP, authentication,
permissions, or launch behavior remain compatible. Removing that mechanism must not remove local
artifact security, process containment, auditability, or strict session continuity.

## Decision

Keep exactly four built-in `ProviderDriver`s. Drivers own launch command/args, environment,
authentication, stable ACP v1, required capabilities, probes, and process workarounds. At each new
Agent startup agentmux:

1. canonicalizes the launch executable and validates type, owner, writable/executable mode;
2. observes SHA-256/file identity and best-effort version/component information;
3. evaluates optional user-owned exact local assertions;
4. rechecks file identity immediately before spawn; and
5. negotiates ACP v1 and operation capabilities at runtime.

Probe failure or an unfamiliar/non-semver version is not a failure unless an assertion needs that
observation. Permission policy is identical for all versions. The resulting
`ProviderExecutionIdentity` is audit and continuity data, never a trust label.

## Consequences

- New provider versions can run without an agentmux data or binary update when the driver contract
  remains intact.
- A breaking command, auth, ACP, permission, or package-layout change may require a driver revision.
- Users who need reproducibility can pin exact local version/component/digest assertions.
- Executable safety, artifact replacement detection, owned-process cleanup, and no-fallback
  continuity remain mandatory.
- Past tested combinations are optional reference information with no runtime effect or maintainer
  commitment to track releases.
- agentmux performs no provider discovery, installation, update, or metadata network request.
