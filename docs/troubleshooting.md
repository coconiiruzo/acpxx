# Troubleshooting

Start with `agentmux doctor --json`, `agentmux provider inspect PROFILE --json`, and
`agentmux list`.

- `provider_spawn_failed`: check the absolute canonical path, regular-file type, current-user/root
  ownership, executable bits, and absence of group/world write bits.
- `provider_assertion_failed`: the configured exact value mismatched or could not be observed.
  Update the local assertion intentionally or delete the assertions table to use runtime
  negotiation.
- `provider_artifact_changed`: the launch artifact or an asserted supplementary metadata file
  changed between observation and spawn; retry only after the local installation is stable.
- `acp_initialize_failed`: the provider did not negotiate stable ACP v1 or lacks a required
  operation capability.
- `authentication_failed`: complete the provider's login/API-key flow before starting the broker.
- `stale_parent` / `agent_busy`: use the latest terminal Run or wait/interrupt the active Run.
- `continuity_lost`: spawn a new Agent; agentmux never creates a replacement session or replays a
  transcript.
- wait timeout: only the client stopped waiting. Reconnect with `wait` or `watch`.
- `host_restarted`: the former live session cannot be proven after restart.
- IPC ownership/version error: stop the owning broker and validate runtime-directory permissions;
  never delete an active socket or lock by guesswork.
- SQLite corruption/newer schema: back up the file and use an explicitly selected database. The
  broker never starts providers as a repair action.

Version probe warnings alone do not prevent a spawn without a matching assertion. Reports should
include redacted doctor/inspect output, failure code/stage, agentmux version, and observed provider
identity—never credentials, prompts, output bodies, session IDs, or raw environment dumps.
