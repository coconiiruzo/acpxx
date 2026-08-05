#![cfg(unix)]

mod support;

use std::time::Duration;

use acpxx::{
    Broker, Continuity, ContinuityLossReason, FailureCode, ProviderId, StopReason,
    TerminalRunState, WaitOptions,
};
use support::MockProviderFixture;

#[tokio::test]
async fn stderr_flood_is_drained_without_deadlock_or_broker_failure() {
    let fixture = MockProviderFixture::new(ProviderId::Grok);
    let broker = Broker::new(1);
    let flooded = broker.spawn(fixture.request("stderr_flood")).await.unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        broker.wait_run(flooded.run, WaitOptions::default()),
    )
    .await
    .expect("provider stderr flood deadlocked the Run")
    .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Succeeded);

    let healthy = broker.spawn(fixture.request("normal")).await.unwrap();
    assert_eq!(
        broker
            .wait_run(healthy.run, WaitOptions::default())
            .await
            .unwrap()
            .state,
        TerminalRunState::Succeeded
    );
    broker.shutdown().await.unwrap();
}

#[tokio::test]
async fn permission_request_transport_disconnect_fails_only_its_agent() {
    let fixture = MockProviderFixture::new(ProviderId::Grok);
    let broker = Broker::new(2);
    let disconnected = broker
        .spawn(fixture.request("permission_disconnect"))
        .await
        .unwrap();
    let receipt = broker
        .wait_run(disconnected.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.as_ref().map(|failure| failure.code),
        Some(FailureCode::ProviderCrashed)
    );
    let snapshot = broker.list(Default::default()).await.unwrap();
    let disconnected_agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.agent_id == disconnected.agent.agent_id)
        .unwrap();
    assert_eq!(
        disconnected_agent.continuity,
        Some(Continuity::Lost(ContinuityLossReason::ProviderExited))
    );

    let healthy = broker.spawn(fixture.request("normal")).await.unwrap();
    assert_eq!(
        broker
            .wait_run(healthy.run, WaitOptions::default())
            .await
            .unwrap()
            .state,
        TerminalRunState::Succeeded
    );
    broker.shutdown().await.unwrap();
}

#[tokio::test]
async fn run_deadline_terminates_an_authentication_hang() {
    let fixture = MockProviderFixture::new_with_marker(ProviderId::Grok, "auth_hang");
    let broker = Broker::new(1);
    let mut request = fixture.request("normal");
    request.task = request.task.with_deadline(Duration::from_millis(50));
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        broker.wait_run(spawned.run, WaitOptions::default()),
    )
    .await
    .expect("deadline did not terminate authentication hang")
    .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Interrupted);
    assert_eq!(receipt.stop_reason, StopReason::DeadlineExceeded);
    let snapshot = broker.list(Default::default()).await.unwrap();
    assert!(matches!(
        snapshot.agents[0].continuity,
        Some(Continuity::Lost(
            ContinuityLossReason::ForcedKill | ContinuityLossReason::ProviderExited
        ))
    ));
    broker.shutdown().await.unwrap();
}
