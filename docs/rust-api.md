# Rust API

The package is `acpxx`; the binary is `agentmux`. `acpxx::Broker` is the in-process entry point.

```rust,no_run
use acpxx::{Broker, PermissionPolicy, ProviderAssertions, ProviderSpec, SpawnRequest, Task,
            WaitOptions};

# async fn example() -> acpxx::Result<()> {
let broker = Broker::new(4);
let spawned = broker.spawn(SpawnRequest {
    provider: ProviderSpec::grok(),
    cwd: std::env::current_dir().unwrap(),
    task: Task::new("Summarize the repository"),
    permission_policy: PermissionPolicy::Deny,
    assertions: ProviderAssertions::default(),
}).await?;
let receipt = broker.wait_run(spawned.run, WaitOptions::default()).await?;
assert!(receipt.provider_identity.is_some());
broker.shutdown().await?;
# Ok(())
# }
```

Control methods are `spawn`, `send`, `followup`, `interrupt`, and `list`; observation methods are
`events`, `wait_run`, `wait_any`, and `wait_all`. Handles contain UUIDv7 IDs and must be returned
exactly. `SpawnRequest.assertions` defaults to no version gate and supports exact version,
components, and launch SHA-256 only.

`ProviderExecutionIdentity` is audit/continuity information: driver ID/revision, target, canonical
path, launch digest, best-effort observations, ACP protocol/agent info/capability digest, and
assertion result. It does not represent trust or a support promise. IPC v2 serializes the same final
field names over a bounded local socket; no HTTP API exists.
