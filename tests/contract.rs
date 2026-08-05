mod support;

use std::path::PathBuf;

use acpxx::{
    AdmissionError, AgentMessage, Broker, ControlError, FollowupTask, ListQuery, PermissionPolicy,
    ProviderId, ProviderSpec, SpawnRequest, Task,
};
use support::mock_request;

#[tokio::test]
async fn public_provider_snapshot_is_the_closed_v1_set() {
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
