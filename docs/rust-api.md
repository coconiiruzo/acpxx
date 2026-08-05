# Rust API

The package name is `acpxx`; the distributed binary is `agentmux`. The primary
in-process entry point is `acpxx::Broker`.

```rust,no_run
use acpxx::{Broker, ProviderSpec, SpawnRequest, Task, WaitOptions};

# async fn example() -> acpxx::Result<()> {
let broker = Broker::new(4);
let spawned = broker.spawn(SpawnRequest {
    provider: ProviderSpec::grok(),
    cwd: std::env::current_dir().unwrap(),
    task: Task::new("Summarize the repository"),
    permission_policy: acpxx::PermissionPolicy::Deny,
}).await?;
let receipt = broker.wait_run(spawned.run, WaitOptions::default()).await?;
assert!(receipt.state.is_terminal());
broker.shutdown().await?;
# Ok(())
# }
```

Control methods are `spawn`, `send`, `followup`, `interrupt`, and `list`.
Observation methods are `events`, `wait_run`, `wait_any`, and `wait_all`.
Handles contain UUIDv7 IDs and must be passed back exactly; display names,
paths, cwd, provider names, and session IDs are never accepted as identity.

The public contract is frozen in [`PRODUCT_CONTRACT.md`](../PRODUCT_CONTRACT.md).
The local daemon uses the same serializable request and response types over a
versioned, bounded UDS protocol; it is not an HTTP API.
