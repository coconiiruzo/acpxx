# v1 Non-goals

Status: frozen for the v1 plan
Last updated: 2026-08-05

The following are deliberately outside the v1 scope:

- active-turn steering through the common API;
- a custom provider plugin system or public arbitrary-command provider;
- session migration between providers or provider fallback;
- transcript replay as simulated continuity;
- automatic session resume after a provider-process or broker restart;
- provider-specific CLI protocol parsing;
- PTY or TUI scraping;
- Web UI, HTTP API, remote broker, or cloud control plane;
- distributed scheduling;
- automatic provider or adapter installation and updates;
- a common cross-provider sandbox guarantee;
- storing prompts, output, reasoning, or complete tool payloads by default; and
- claims that Rust orchestration accelerates model inference.

An internal fake ACP executable is required for deterministic runtime and
conformance tests. It is a test fixture, not a supported provider or extension
point. Future ideas do not receive empty modules, enum variants, compatibility
rows, or placeholder APIs in v1.
