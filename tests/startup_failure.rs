use std::path::PathBuf;

use acpxx::{
    Broker, FailureCode, PermissionPolicy, ProcessDisposition, ProviderSpec, SpawnRequest, Task,
    TerminalRunState, WaitOptions,
};
use uuid::Uuid;

#[tokio::test]
async fn missing_provider_fails_through_wait_run() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(SpawnRequest {
            provider: ProviderSpec::Grok {
                executable: Some(PathBuf::from("agentmux-provider-that-does-not-exist")),
            },
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            task: Task::new("test"),
            permission_policy: PermissionPolicy::Deny,
            version_policy: acpxx::VersionPolicy::Experimental,
            catalog_entry: None,
            allow_unverified_mutations: false,
        })
        .await
        .unwrap();

    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.unwrap().code,
        FailureCode::ProviderSpawnFailed
    );
    assert_eq!(
        receipt.cleanup.process,
        acpxx::ProcessDisposition::NeverStarted
    );
}

#[tokio::test]
async fn missing_cursor_binary_fails_through_the_run_receipt() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(SpawnRequest {
            provider: ProviderSpec::Cursor {
                executable: Some(PathBuf::from("agentmux-cursor-that-does-not-exist")),
            },
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            task: Task::new("test"),
            permission_policy: PermissionPolicy::Deny,
            version_policy: acpxx::VersionPolicy::Experimental,
            catalog_entry: None,
            allow_unverified_mutations: false,
        })
        .await
        .unwrap();

    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.unwrap().code,
        FailureCode::ProviderSpawnFailed
    );
}

#[cfg(unix)]
#[tokio::test]
async fn missing_required_auth_method_is_an_authentication_failure() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
    let link = std::env::temp_dir().join(format!("agentmux-no_auth-{}", Uuid::now_v7()));
    std::fs::hard_link(&fixture, &link).unwrap();
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(SpawnRequest {
            provider: ProviderSpec::Grok {
                executable: Some(link.clone()),
            },
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            task: Task::new("test"),
            permission_policy: PermissionPolicy::Deny,
            version_policy: acpxx::VersionPolicy::Experimental,
            catalog_entry: None,
            allow_unverified_mutations: false,
        })
        .await
        .unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.unwrap().code,
        FailureCode::AuthenticationFailed
    );
    assert_eq!(receipt.cleanup.process, ProcessDisposition::Terminated);
    let _ = std::fs::remove_file(link);
}
