# CLI reference

Client commands use the current user's mode-0600 Unix Domain Socket. `--socket` and `--config` are
global overrides. Structured responses are JSON; `watch` emits JSON Lines.

| Command | Purpose |
|---|---|
| `serve` | Start the broker and SQLite v3 metadata store |
| `spawn` | Create an Agent, live ACP session, and initial Run |
| `send AGENT MESSAGE` | Queue content without creating/steering a Run |
| `followup AGENT --after RUN TASK` | Start the next same-session Run |
| `interrupt RUN` | Cancel one Run with owned-process escalation |
| `list` | Read Agent, Run, driver, identity, and capability snapshots |
| `wait`, `wait-any`, `wait-all` | Event-driven terminal receipt waits |
| `watch RUN` | Stream ordered Run events and receipt |
| `provider inspect PROFILE [--json]` | Observe local safety/version/digest/assertions without ACP |
| `config migrate --check\|--write [--json]` | Inspect or atomically write final config schema v2 |
| `doctor [--json]` | Audit config, artifacts, observations, auth hints, IPC, and SQLite |
| `benchmark [--enforce] [--json]` | Measure broker overhead independently from model work |

```bash
agentmux serve --max-concurrency 8 --provider-limit codex=2
agentmux spawn --profile codex-default --cwd /repo --deadline-ms 120000 \
  "Investigate the failing tests"
agentmux send AGENT_UUID "Do not alter database migrations"
agentmux wait RUN_UUID --timeout-ms 30000
agentmux followup AGENT_UUID --after RUN_UUID "Run remaining tests"
agentmux watch NEXT_RUN_UUID
```

A wait timeout or client disconnect never cancels broker work. An execution deadline uses the Run
interrupt path. `provider inspect` never opens an ACP session; use a real spawn or the authenticated
conformance suite to verify negotiation. The installed command's `--help` is authoritative for
options.
