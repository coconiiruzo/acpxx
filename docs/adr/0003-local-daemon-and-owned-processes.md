# ADR-0003: Local daemon, UDS, and owned process supervision

- Status: accepted for v1
- Date: 2026-08-05

## Context

Persistent Agents must survive individual CLI client exits. Provider descendants
must also be collected after normal shutdown, failed cancellation, broker
termination, and abrupt parent death without guessing ownership from process
names, paths, or PIDs.

## Decision

Ship one Rust binary with CLI-client, broker, and hidden supervisor modes. On
macOS, clients communicate with one per-user broker through a mode-0600 Unix
Domain Socket with a version handshake, bounded frames, and a single-broker
lock. v1 exposes no TCP or HTTP listener.

The broker starts the same binary in supervisor mode through a watchdog pipe.
The supervisor owns a provider process group and terminates and reaps that group
when the watchdog closes. Normal interruption first attempts ACP cancel and then
escalates through owned handles after bounded grace periods.

## Consequences

- Closing a CLI connection does not cancel its Run.
- The broker owns only processes it created and retained handles for.
- `pkill`, `killall`, command-name lookup, cwd lookup, and inferred PIDs are
  prohibited.
- macOS arm64 is the required v1 target; additional platform containment is a
  later release decision.
