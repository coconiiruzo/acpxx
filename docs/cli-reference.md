# CLI reference

All client commands use the current user's Unix Domain Socket. Override it with
the global `--socket` option. Structured commands emit JSON; `watch` emits JSON
Lines events followed by the terminal receipt.

| Command | Purpose |
| --- | --- |
| `serve` | Start the local broker and SQLite metadata store |
| `spawn` | Create an Agent, live ACP session, and initial Run |
| `send AGENT MESSAGE` | Queue mailbox content without starting a Run |
| `followup AGENT --after RUN TASK` | Start the next same-session Run |
| `interrupt RUN` | Request cancellation of one Run |
| `list` | Read Agent, Run, provider, and capability snapshots |
| `wait RUN` | Wait for one terminal receipt |
| `wait-any RUN...` | Return the first terminal Run by completion sequence |
| `wait-all RUN...` | Return all receipts in argument order |
| `watch RUN` | Stream ordered Run events and its receipt |
| `doctor --json` | Audit configuration, versions, auth, IPC, and SQLite |
| `compatibility status [--json]` | Show the active signed Catalog, sequence, expiry, and recommendations |
| `compatibility update [--file CATALOG --signature SIG]` | Install a higher signed Catalog and reload the daemon |
| `provider status [--profile NAME] [--json]` | Probe configured artifacts without spawning ACP sessions |
| `provider verify PROFILE [--json]` | Resolve one profile against the active Catalog without spawning |
| `config migrate --check` | Dry-run schema-v1 to schema-v2 migration |
| `config migrate --write` | Backup and atomically write a qualified schema-v2 config |
| `benchmark --enforce --json` | Run the reference performance qualification |

Typical flow:

```bash
agentmux serve --max-concurrency 8 --provider-limit codex=2
agentmux spawn --profile codex-default --cwd /repo \
  --deadline-ms 120000 "Investigate the failing tests"
agentmux send AGENT_UUID "Do not alter database migrations"
agentmux wait RUN_UUID --timeout-ms 30000
agentmux followup AGENT_UUID --after RUN_UUID "Run the remaining tests"
agentmux watch NEXT_RUN_UUID
agentmux compatibility status --json
agentmux provider verify codex-default --json
```

`--timeout-ms` on a wait only stops that client waiting. It never interrupts a
Run. `--deadline-ms` is an execution deadline and uses the Run interruption
path. A client disconnect never cancels broker-owned work. Use each command's
`--help` output as the exact option reference for the installed version.

`compatibility update` is the only normal command that accesses the network.
`serve`, `spawn`, `provider verify`, and Run operations are offline. Import an
air-gapped signed Catalog with `--file` and `--signature`.
