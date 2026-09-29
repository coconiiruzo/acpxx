mod support;

use std::time::Duration;

use acpxx::{
    Broker, FailureCode, ListQuery, PermissionPolicy, RunStage, TerminalRunState, WaitOptions,
};
use support::mock_request;

#[tokio::test]
async fn persistent_run_streams_output_and_retains_the_session() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
    assert_eq!(receipt.output.text, "mock-ok");
    assert_eq!(receipt.output.event_count, 1);
    assert!(receipt.cleanup.complete);
    assert_eq!(receipt.cleanup.process, acpxx::ProcessDisposition::Retained);
    assert!(receipt.metrics.provider_probe > Duration::ZERO);
    assert!(receipt.metrics.adapter_spawn > Duration::ZERO);
    assert!(receipt.metrics.acp_initialize > Duration::ZERO);
    assert!(receipt.metrics.authentication > Duration::ZERO);
    assert!(receipt.metrics.session_new > Duration::ZERO);
    assert!(receipt.metrics.first_output > Duration::ZERO);
    assert!(receipt.metrics.model_and_tools >= receipt.metrics.first_output);
    assert!(receipt.metrics.cleanup > Duration::ZERO);
    assert!(receipt.metrics.total >= receipt.metrics.model_and_tools);

    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(snapshot.agents[0].process_alive);
    assert!(matches!(
        snapshot.agents[0].continuity,
        Some(acpxx::Continuity::Available(_))
    ));
    assert!(snapshot.agents[0].provider_capabilities.is_some());
    assert_eq!(snapshot.agents[0].active_run_id, None);
}

#[tokio::test]
async fn provider_crash_is_a_terminal_failure() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("crash", 0.0)).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Failed);
    let failure = receipt.failure.unwrap();
    assert_eq!(failure.code, FailureCode::ProviderCrashed, "{failure:?}");
    assert!(receipt.cleanup.complete);
}

#[tokio::test]
async fn wait_timeout_does_not_cancel_the_run() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.15)).await.unwrap();
    let timed_out = broker
        .wait_run(
            spawned.run,
            WaitOptions {
                timeout: Some(Duration::from_millis(10)),
            },
        )
        .await;
    assert!(matches!(
        timed_out,
        Err(acpxx::ControlError::WaitTimeout { .. })
    ));

    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn running_snapshot_projects_process_and_acp_stage() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.15)).await.unwrap();
    let snapshot = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = broker
                .list(ListQuery {
                    run_id: Some(spawned.run.run_id),
                    ..ListQuery::default()
                })
                .await
                .unwrap();
            if snapshot.runs[0].stage == RunStage::Prompting {
                break snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("run must reach prompting stage");
    assert!(snapshot.agents[0].process_alive);
    assert_eq!(snapshot.runs[0].stage, RunStage::Prompting);

    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn malformed_stdout_is_protocol_corruption() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("malformed", 0.0)).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.unwrap().code,
        FailureCode::ProtocolCorruption
    );
}

#[tokio::test]
async fn scripted_prompt_failure_is_terminal() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("prompt_failure", 0.0))
        .await
        .unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(receipt.failure.unwrap().code, FailureCode::PromptFailed);
}

#[tokio::test]
async fn scripted_transport_close_is_terminal() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("transport_close", 0.0))
        .await
        .unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(receipt.failure.unwrap().code, FailureCode::ProviderCrashed);
}

#[tokio::test]
async fn permission_request_does_not_deadlock_dispatch() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("permission", 0.0)).await.unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(2),
        broker.wait_run(spawned.run, WaitOptions::default()),
    )
    .await
    .expect("permission callback must not deadlock")
    .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn deny_policy_cancels_only_when_no_reject_option_is_advertised() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("permission_without_reject", 0.0))
        .await
        .unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(2),
        broker.wait_run(spawned.run, WaitOptions::default()),
    )
    .await
    .expect("permission callback must not deadlock")
    .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn explicit_allow_policy_selects_the_advertised_permission() {
    let broker = Broker::new(1);
    let mut request = mock_request("permission_allow", 0.0);
    request.permission_policy = PermissionPolicy::AllowAll;
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(2),
        broker.wait_run(spawned.run, WaitOptions::default()),
    )
    .await
    .expect("permission callback must not deadlock")
    .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn terminal_host_services_execute_inside_the_agent_root() {
    let broker = Broker::new(1);
    let mut request = mock_request("terminal", 0.0);
    request.permission_policy = PermissionPolicy::AllowAll;
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(2),
        broker.wait_run(spawned.run, WaitOptions::default()),
    )
    .await
    .expect("terminal callbacks must not deadlock")
    .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
    assert_eq!(receipt.output.text, "terminal-ok");
}

#[tokio::test]
async fn deny_policy_neither_advertises_nor_serves_the_terminal_host() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("terminal_denied", 0.0))
        .await
        .unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(2),
        broker.wait_run(spawned.run, WaitOptions::default()),
    )
    .await
    .expect("terminal callbacks must not deadlock")
    .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("terminal-denied.txt")
            .exists()
    );
}

#[tokio::test]
async fn output_flood_is_bounded() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("output_flood", 0.0))
        .await
        .unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Succeeded);
    assert!(receipt.output.truncated);
    assert_eq!(receipt.output.text.len(), 8 * 1024 * 1024);
    assert_eq!(receipt.output.event_count, 9000);
}

#[tokio::test]
async fn already_terminal_run_is_observed_without_a_lost_notification() {
    let broker = Broker::new(8);
    let mut runs = Vec::new();
    for _ in 0..32 {
        runs.push(broker.spawn(mock_request("normal", 0.0)).await.unwrap().run);
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    for run in runs {
        let receipt = broker
            .wait_run(
                run,
                WaitOptions {
                    timeout: Some(Duration::from_secs(2)),
                },
            )
            .await
            .unwrap();
        assert_eq!(receipt.state, TerminalRunState::Succeeded);
    }
}
