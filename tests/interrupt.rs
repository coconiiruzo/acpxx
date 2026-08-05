mod support;

use std::time::Duration;

use acpxx::{Broker, Continuity, FollowupTask, ListQuery, RunStage, TerminalRunState, WaitOptions};
use support::mock_request;

#[tokio::test]
async fn graceful_cancel_preserves_continuity() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("cancel_success", 0.0))
        .await
        .unwrap();
    wait_for_prompting(&broker, spawned.run).await;
    let interrupt = broker.interrupt(spawned.run).await.unwrap();
    assert!(interrupt.requested);
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Interrupted);
    assert!(!broker.interrupt(spawned.run).await.unwrap().requested);

    let followup = broker
        .followup(
            spawned.agent,
            spawned.run.run_id,
            FollowupTask::new("continue after cancel"),
        )
        .await
        .unwrap();
    let receipt = broker
        .wait_run(followup, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Succeeded);
}

#[tokio::test]
async fn ignored_cancel_forces_cleanup_and_loses_continuity() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("cancel_ignore", 0.0))
        .await
        .unwrap();
    wait_for_prompting(&broker, spawned.run).await;
    broker.interrupt(spawned.run).await.unwrap();
    let receipt = broker
        .wait_run(
            spawned.run,
            WaitOptions {
                timeout: Some(Duration::from_secs(3)),
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Interrupted);
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
    assert!(matches!(
        snapshot.agents[0].continuity,
        Some(Continuity::Lost(acpxx::ContinuityLossReason::ForcedKill))
    ));
}

#[tokio::test]
async fn run_deadline_uses_the_interrupt_path_without_becoming_a_wait_timeout() {
    let broker = Broker::new(1);
    let mut request = mock_request("cancel_success", 5.0);
    request.task = request.task.with_deadline(Duration::from_millis(150));
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Interrupted);
    assert_eq!(receipt.stop_reason, acpxx::StopReason::DeadlineExceeded);
    let agent = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(
        matches!(agent.agents[0].continuity, Some(Continuity::Available(_))),
        "continuity was {:?}",
        agent.agents[0].continuity
    );
}

async fn wait_for_prompting(broker: &Broker, run: acpxx::RunHandle) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = broker
                .list(ListQuery {
                    run_id: Some(run.run_id),
                    ..ListQuery::default()
                })
                .await
                .unwrap();
            if snapshot.runs[0].stage == RunStage::Prompting {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
