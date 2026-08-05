mod support;

use std::time::Duration;

use acpxx::{Broker, FailureCode, ListQuery, TerminalRunState, WaitOptions};
use support::mock_request;

#[tokio::test]
async fn shutdown_fails_an_active_run_and_loses_continuity() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("shutdown", 5.0)).await.unwrap();

    loop {
        let snapshot = broker
            .list(ListQuery {
                run_id: Some(spawned.run.run_id),
                ..ListQuery::default()
            })
            .await
            .unwrap();
        if snapshot.runs[0].stage == acpxx::RunStage::Prompting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    broker.shutdown().await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.as_ref().map(|failure| failure.code),
        Some(FailureCode::HostShutdown)
    );

    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(!snapshot.agents[0].process_alive);
    assert_eq!(
        snapshot.agents[0].continuity,
        Some(acpxx::Continuity::Lost(
            acpxx::ContinuityLossReason::HostShutdown
        ))
    );
}
