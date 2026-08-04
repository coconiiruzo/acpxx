# ADR-0001: Handle-first API over stable ACP v1

- Status: accepted
- Date: 2026-08-05

## Context

Provider CLIs expose different process, session, streaming, permission, and
cancellation surfaces. Using paths, display names, PIDs, or provider session IDs
as public identity makes orchestration ambiguous and leaks transport details.

## Decision

Expose opaque UUIDv7 `AgentId` and `RunId` handles. Keep provider transport on
ACP and start with stable protocol v1. Provider modules may select executables,
arguments, tested versions, environment names, and required capabilities, but
must not implement provider-specific wire parsing.

One logical Agent owns one provider process, one ACP connection, one ACP
session, and at most one foreground Run. A Run uses only the five canonical
states in the domain model.

## Consequences

- API targets cannot be resolved from names or paths.
- Provider startup failure can use the same Run receipt path as model failure.
- Cross-provider behavior is limited to what ACP can represent.
- Stable v1 ships before draft v2 support.
- Provider integration changes should normally be manifest and conformance-test
  additions, not runtime changes.
