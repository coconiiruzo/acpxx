# Troubleshooting

Start with:

```bash
agentmux doctor --json
agentmux list
```

Common failures:

- `adapter_not_found` or `provider_spawn_failed`: configure an absolute path and
  confirm it is executable.
- version mismatch: install the exact compatibility-manifest version; agentmux
  deliberately does not auto-update or fall back.
- `authentication_failed`: complete the provider's own login flow or supply its
  documented credential environment before starting the broker.
- `stale_parent`: use the Agent's latest terminal Run ID from `agentmux list`.
- `agent_busy`: wait for or interrupt the active Run; one Agent permits one Run.
- `continuity_lost`: spawn a new Agent. Replacement sessions and transcript
  replay are intentionally forbidden.
- wait timeout: the Run is still executing. Inspect `list` or reconnect with
  `wait`/`watch`.
- `host_restarted`: the old live process/session cannot be proven after restart.
- socket permission/version error: remove neither a live socket nor lock by
  guesswork; stop the owning broker, verify the current-user runtime directory,
  and rerun `doctor`.
- SQLite corruption: back up the file and start with an explicitly selected new
  database. A corrupt database never causes providers to start.

Provider stderr is drained with a bounded tail by the pinned ACP SDK. ACP stdout
must contain protocol frames only. For a repeatable report, include agentmux and
provider versions, the redacted `doctor --json` output, failure code/stage, and
the compatibility manifest. Never attach credentials, prompt/output bodies, or
raw environment dumps.
