# Performance Qualification

agentmux reports broker overhead separately from provider startup, ACP
initialization, authentication, and model/tool execution. Model latency is not
an agentmux performance result.

## Command

Build and measure the release binary on an Apple silicon Mac Studio:

```bash
cargo build --release
target/release/agentmux benchmark --enforce --json
```

`--enforce` is accepted only by a release build running on macOS arm64 hardware
reported as `Mac Studio`. Debug builds and other hosts still produce a report,
but `qualification_eligible` is false and `passed` is null. `--samples` and
`--events` may reduce a diagnostic run; the frozen defaults are 100 samples and
100,000 synthetic events.

## Frozen budgets

| Measurement | Budget |
| --- | ---: |
| cold process startup p95 | 50 ms |
| broker idle RSS | 30 MiB |
| in-process command admission p99 | 2 ms |
| IPC command admission p99 | 5 ms |
| terminal-style watch wake p99 | 5 ms |
| bounded synthetic event fan-in | 10,000 events/sec, no loss |

The command exits unsuccessfully under `--enforce` when any threshold or event
integrity check fails. Budget changes require measurement evidence and an ADR.

## Methodology

- Cold startup launches the current `agentmux --version` binary once for warmup
  and then 100 independent times. Compilation is excluded.
- Idle RSS samples the benchmark process after constructing an empty Broker.
- In-process admission measures a warmed `Broker::list` command.
- IPC admission opens a fresh Unix Domain Socket connection for each warmed
  `list` request, including framing and serialization.
- Waiter wake measures the same Tokio watch notification primitive used for
  terminal Run observation.
- Event fan-in uses eight concurrent producers, a bounded channel, and an exact
  sequence bitmap. Missing and duplicate events fail the run.

These synthetic measurements intentionally exclude provider processes and LLM
work. Per-Run receipts separately expose provider probe, adapter spawn, ACP
initialize, authentication, session creation, first output, model/tool, and
cleanup timings.

## Baseline — 2026-08-05

Reference host: Mac Studio (`Mac16,9`), Apple M4 Max, 128 GB RAM, macOS 26.2,
Rust 1.96.0, release profile with thin LTO and stripped symbols.

| Measurement | Result |
| --- | ---: |
| cold startup p95 | 2.690 ms |
| broker idle RSS | 4.188 MiB |
| in-process admission p99 | 0.0029 ms |
| IPC admission p99 | 0.0415 ms |
| waiter wake p99 | 0.0073 ms |
| synthetic event fan-in | 7,996,588 events/sec |
| delivered events | 100,000 / 100,000 |

All frozen synthetic budgets passed. The fake 10,000-Run lifecycle soak passed
in 2.69 seconds with no state or FD leak, and the real 1,000-process supervisor
soak passed in 4.06 seconds with no zombie or FD leak. Chaos coverage verifies
malformed stdout, 2 MiB stderr flood, hung authentication, permission-time
transport loss, ignored cancel, child/grandchild hangs, SQLite write failure,
socket disconnect, and broker shutdown without a broker-wide panic. Phase 17 is
complete on the reference host.
