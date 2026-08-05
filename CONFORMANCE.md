# Provider Conformance

Provider promotion uses two complementary black-box suites. Both resolve an
exact signed Catalog entry through the provider's `ProviderDriver`; neither
parses a provider-private CLI protocol.

## Authenticated provider suite

`tests/provider_conformance.rs` runs the pinned, authenticated executable and
checks:

- exact version probe, initialize, authentication, and `session/new`;
- ordered streaming events and a terminal first prompt;
- at least three turns with an unchanged session stamp;
- no output duplication across Run boundaries;
- cancellation after the first streamed output and same-session recovery;
- capability snapshot publication; and
- broker shutdown state and process disposition.

These tests are ignored by default because they require local credentials and
may consume model quota. They resolve the 0600 pinned profile file when an
explicit executable environment variable is absent.

```bash
cargo test --test provider_conformance -- --ignored --test-threads=1
```

## Controlled fault suite

`tests/provider_fault_conformance.rs` connects a scriptable ACP process through
each of the four provider Drivers under a test Catalog. It checks:

- provider crash classification;
- malformed protocol classification;
- strict no-fallback behavior after continuity loss;
- permission deny and explicit allow responses plus event projection; and
- owned descendant cleanup using the PID created by the fixture.

```bash
cargo test --test provider_fault_conformance
```

The controlled suite proves broker behavior under faults that a conforming real
provider cannot be asked to emit deliberately. It does not replace conditional
tests for capabilities advertised by a real provider, nor the real executable's
process-tree escape audit. Those checks remain required before `stable`
promotion.

## Real process-tree audit

`tests/provider_process_audit.rs` starts the packaged daemon and one pinned real
provider profile. It discovers descendants only by walking parent PID
relationships from the `Child` handle owned by the test. After sending SIGKILL
to the broker, every observed supervisor/provider descendant must disappear via
the watchdog path. It never searches or kills by process name.

```bash
cargo test --test provider_process_audit -- --ignored --test-threads=1
```

## Capability inventory

`tests/provider_capability_audit.rs` freezes the stable-v1 capability fields
advertised by each pinned executable. `AgentSnapshot` exposes those raw provider
capabilities beside a typed `broker_capabilities` value. Optional image, audio,
embedded-context, MCP, session-load, and session-resume operations remain
explicitly `false` on the broker side because they are outside the frozen v1
Control API; agentmux does not emulate them or silently claim support.

```bash
cargo test --test provider_capability_audit -- --ignored
```

## Real permission audit

`tests/provider_permission_audit.rs` runs authenticated Codex and Claude
mutations against isolated paths. For each adapter it verifies that:

- deny emits `PermissionRequested` and does not create the target file;
- allow-all selects an ACP `allow_once`/`allow_always` option by kind, not by
  array position, and writes the exact fixture content; and
- namespaced provider `_meta` survives in the bounded, secret-redacted event
  envelope.

Codex's tested Catalog entry fixes `INITIAL_AGENT_MODE=read-only`, disables Guardian
auto-approval, and sets `approvals_reviewer=user`; otherwise Codex 0.145.0 can
approve a mutation before emitting an ACP permission request.

```bash
cargo test --test provider_permission_audit -- --ignored
```

## Stable promotion rule

A provider is `stable` only when:

1. its pinned executable passes the authenticated suite;
2. its Driver passes every controlled fault case;
3. every advertised optional capability is either tested through the broker or
   explicitly reported as unsupported by the frozen broker capability snapshot;
   and
4. the real executable's descendants are all reaped during the process-tree
   audit.

Failure never triggers a new session, another provider, transcript replay, or
automatic installation/update.
