mod support;

use std::time::Duration;

use acpxx::{Broker, NonEmpty, TerminalRunState, WaitOptions};
use support::mock_request;

#[tokio::test]
async fn wait_any_returns_first_completion_without_cancelling_others() {
    let broker = Broker::new(2);
    let slow = broker.spawn(mock_request("normal", 0.2)).await.unwrap();
    wait_for_stage(&broker, slow.run, acpxx::RunStage::Prompting).await;
    let fast = broker.spawn(mock_request("normal", 0.02)).await.unwrap();
    let mut runs = NonEmpty::new(slow.run);
    runs.tail.push(fast.run);

    let first = broker.wait_any(runs, WaitOptions::default()).await.unwrap();
    assert_eq!(first.run_id, fast.run.run_id);
    assert_eq!(first.state, TerminalRunState::Succeeded);

    let slow_receipt = broker
        .wait_run(slow.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(slow_receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn wait_all_preserves_input_order() {
    let broker = Broker::new(2);
    let slow = broker.spawn(mock_request("normal", 0.15)).await.unwrap();
    wait_for_stage(&broker, slow.run, acpxx::RunStage::Prompting).await;
    let fast = broker.spawn(mock_request("normal", 0.01)).await.unwrap();
    let mut runs = NonEmpty::new(slow.run);
    runs.tail.push(fast.run);

    let receipts = broker.wait_all(runs, WaitOptions::default()).await.unwrap();
    assert_eq!(receipts[0].run_id, slow.run.run_id);
    assert_eq!(receipts[1].run_id, fast.run.run_id);
    assert!(receipts[1].completion_sequence < receipts[0].completion_sequence);
}

#[tokio::test]
async fn aggregate_wait_timeout_does_not_cancel_runs() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.1)).await.unwrap();
    let result = broker
        .wait_all(
            NonEmpty::new(spawned.run),
            WaitOptions {
                timeout: Some(Duration::from_millis(5)),
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(acpxx::ControlError::WaitTimeout { .. })
    ));

    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn one_hundred_agents_complete_without_lost_wait_notifications() {
    let broker = Broker::new(16);
    let mut handles = Vec::new();
    for _ in 0..100 {
        handles.push(broker.spawn(mock_request("normal", 0.0)).await.unwrap().run);
    }
    let mut runs = NonEmpty::new(handles[0]);
    runs.tail.extend_from_slice(&handles[1..]);
    let receipts = broker
        .wait_all(
            runs,
            WaitOptions {
                timeout: Some(Duration::from_secs(10)),
            },
        )
        .await
        .unwrap();
    assert_eq!(receipts.len(), 100);
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt.state == TerminalRunState::Succeeded)
    );
}

#[tokio::test]
async fn followup_waits_for_global_capacity_and_queued_interrupt_sends_no_cancel() {
    let broker = Broker::new(1);
    let first = broker.spawn(mock_request("first", 0.0)).await.unwrap();
    broker
        .wait_run(first.run, WaitOptions::default())
        .await
        .unwrap();

    let blocker = broker.spawn(mock_request("blocker", 0.25)).await.unwrap();
    wait_for_stage(&broker, blocker.run, acpxx::RunStage::Prompting).await;
    let followup = broker
        .followup(
            first.agent,
            first.run.run_id,
            acpxx::FollowupTask::new("queued followup"),
        )
        .await
        .unwrap();
    let snapshot = broker
        .list(acpxx::ListQuery {
            run_id: Some(followup.run_id),
            ..acpxx::ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.runs[0].state, acpxx::RunState::Queued);

    broker.interrupt(followup).await.unwrap();
    let interrupted = broker
        .wait_run(followup, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(interrupted.state, TerminalRunState::Interrupted);
    assert_eq!(
        broker
            .wait_run(blocker.run, WaitOptions::default())
            .await
            .unwrap()
            .state,
        TerminalRunState::Succeeded
    );
    let agent = broker
        .list(acpxx::ListQuery {
            agent_id: Some(first.agent.agent_id),
            ..acpxx::ListQuery::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        agent.agents[0].continuity,
        Some(acpxx::Continuity::Available(_))
    ));
}

async fn wait_for_stage(broker: &Broker, run: acpxx::RunHandle, stage: acpxx::RunStage) {
    loop {
        let snapshot = broker
            .list(acpxx::ListQuery {
                run_id: Some(run.run_id),
                ..acpxx::ListQuery::default()
            })
            .await
            .unwrap();
        if snapshot.runs[0].stage == stage {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
