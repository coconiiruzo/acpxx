# v2 Non-goals

The following are intentionally outside v2:

- ACP v2, active-turn steering, session resume after process/broker restart, provider/session
  migration, provider fallback, or transcript replay as continuity;
- arbitrary/custom provider plugins, registries, user-defined launch args/env, or placeholder
  provider variants beyond Codex, Claude, Grok, and Cursor;
- provider release discovery, update notifications, auto-install/update, version channels/ranges,
  blocklists, vulnerability feeds, or remote telemetry;
- provider-private CLI parsing, PTY/TUI scraping, or provider subagents becoming independent
  agentmux Agents;
- Web UI, HTTP/TCP API, remote/cloud broker, distributed scheduling, or additional release targets;
- a generalized policy engine or cross-provider sandbox/network guarantee;
- default persistence of prompts, outputs, reasoning, or complete tool payloads; and
- claims that Rust orchestration accelerates model inference.

The scriptable ACP fixture is test infrastructure, not a public provider or extension point.
Future ideas receive no empty modules, enum values, compatibility rows, or public shims.
