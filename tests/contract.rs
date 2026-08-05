mod support;

use std::path::PathBuf;

use acpxx::{
    AdmissionError, AgentMessage, Broker, ControlError, FollowupTask, ListQuery, PermissionPolicy,
    ProviderId, ProviderSpec, SpawnRequest, Task,
};
use support::mock_request;

#[tokio::test]
async fn public_provider_snapshot_is_the_closed_four_provider_set() {
    let snapshot = Broker::default().list(ListQuery::default()).await.unwrap();
    let providers: Vec<_> = snapshot
        .providers
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        providers,
        vec![
            ProviderId::Codex,
            ProviderId::Claude,
            ProviderId::Grok,
            ProviderId::Cursor,
        ]
    );
}

#[tokio::test]
async fn public_json_uses_runtime_identity_and_has_no_central_authorization_fields() {
    let request = mock_request("normal", 0.0);
    let request_json = serde_json::to_string(&request).unwrap();
    assert!(!request_json.contains("version_policy"));
    assert!(!request_json.contains("catalog_entry"));
    assert!(!request_json.contains("allow_unverified_mutations"));

    let broker = Broker::default();
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, acpxx::WaitOptions::default())
        .await
        .unwrap();
    let receipt_json = serde_json::to_string(&receipt).unwrap();
    assert!(receipt_json.contains("provider_identity"));
    assert!(!receipt_json.contains("provider_lock"));
    for removed in [
        "catalog_entry",
        "catalog_sequence",
        "catalog_digest",
        "compatibility_level",
        "recommended",
    ] {
        assert!(
            !receipt_json.contains(removed),
            "found removed field {removed}"
        );
    }
    let list_json =
        serde_json::to_string(&broker.list(ListQuery::default()).await.unwrap()).unwrap();
    assert!(!list_json.contains("catalog"));
    assert!(!list_json.contains("recommended"));
    broker.shutdown().await.unwrap();
}

#[test]
fn cli_help_has_no_removed_authorization_commands_or_flags() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout)
        .unwrap()
        .to_ascii_lowercase();
    for removed in [
        "compatibility update",
        "catalog",
        "--version-policy",
        "--catalog-entry",
        "--allow-unverified-mutations",
    ] {
        assert!(
            !help.contains(removed),
            "help contains removed term {removed}"
        );
    }
    assert!(help.contains("provider"));
    assert!(help.contains("config"));
}

#[cfg(unix)]
#[test]
fn provider_inspect_reports_an_unknown_observed_version() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture =
        support::MockProviderFixture::new_with_marker(ProviderId::Grok, "future_version_inspect");
    let config = std::env::temp_dir().join(format!(
        "agentmux-inspect-config-{}.toml",
        uuid::Uuid::now_v7()
    ));
    std::fs::write(
        &config,
        format!(
            "schema_version = 2\n[profiles.future]\nprovider = \"grok\"\nexecutable = \"{}\"\n",
            fixture.executable().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "provider",
            "inspect",
            "future",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let identity: acpxx::ProviderExecutionIdentity =
        serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        identity.observed_version.observed().map(String::as_str),
        Some("9999.0.0")
    );
    assert!(matches!(
        identity.assertion_result,
        acpxx::AssertionResult::NotConfigured
    ));
    std::fs::remove_file(config).unwrap();
}

#[tokio::test]
async fn send_creates_no_run_and_followup_stub_creates_no_run() {
    let broker = Broker::default();
    let spawned = broker.spawn(mock_request("normal", 0.2)).await.unwrap();
    let before = broker.list(ListQuery::default()).await.unwrap().runs.len();

    let send = broker.send(
        spawned.agent,
        AgentMessage {
            content: "queued later".into(),
        },
    );
    let message = tokio::time::timeout(std::time::Duration::from_millis(100), send)
        .await
        .expect("send must be accepted while the Run is active")
        .unwrap();
    assert_eq!(message.accepted_sequence, 1);
    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.agents[0].mailbox_depth, 1);

    let followup = broker
        .followup(
            spawned.agent,
            spawned.run.run_id,
            FollowupTask::new("continue"),
        )
        .await;
    assert!(matches!(
        followup,
        Err(ControlError::Admission(AdmissionError::AgentBusy(_)))
    ));
    assert_eq!(
        broker.list(ListQuery::default()).await.unwrap().runs.len(),
        before
    );
}

#[tokio::test]
async fn invalid_cwd_is_an_admission_error_and_creates_no_run() {
    let broker = Broker::default();
    let result = broker
        .spawn(SpawnRequest {
            provider: ProviderSpec::Grok { executable: None },
            cwd: PathBuf::from("/agentmux/path/that/does/not/exist"),
            task: Task::new("test"),
            permission_policy: PermissionPolicy::Deny,
            assertions: Default::default(),
        })
        .await;
    assert!(matches!(
        result,
        Err(ControlError::Admission(AdmissionError::InvalidCwd { .. }))
    ));
    assert!(
        broker
            .list(ListQuery::default())
            .await
            .unwrap()
            .runs
            .is_empty()
    );
}
