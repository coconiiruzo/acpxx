mod support;

use std::time::Duration;

use acpxx::{Broker, FailureCode, ListQuery, RunStage, TerminalRunState, WaitOptions};
use support::mock_request;

#[tokio::test]
async fn one_shot_run_streams_output_and_publishes_after_cleanup() {
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
    assert_eq!(
        receipt.cleanup.process,
        acpxx::ProcessDisposition::Terminated
    );

    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(!snapshot.agents[0].process_alive);
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
