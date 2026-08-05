mod support;

use std::time::Duration;

use acpxx::{Broker, DiagnosticLevel, RunEventKind, WaitOptions};
use support::mock_request;
use tokio_stream::StreamExt;

#[tokio::test]
async fn output_is_projected_to_a_sequenced_run_stream() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.1)).await.unwrap();
    let mut events = broker.events(spawned.run).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .expect("event stream timed out")
        .expect("event stream closed");
    assert_eq!(event.run_id, spawned.run.run_id);
    assert_eq!(event.agent_id, spawned.agent.agent_id);
    assert!(event.seq > 0);
    assert_eq!(
        event.kind,
        RunEventKind::OutputDelta {
            content: "mock-ok".into()
        }
    );

    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn completion_sequence_is_global_across_agents() {
    let broker = Broker::new(2);
    let first = broker.spawn(mock_request("normal", 0.1)).await.unwrap();
    let second = broker.spawn(mock_request("normal", 0.1)).await.unwrap();
    let mut first_events = broker.events(first.run).unwrap();
    let mut second_events = broker.events(second.run).unwrap();

    let first_event = first_events.next().await.unwrap();
    let second_event = second_events.next().await.unwrap();
    assert_ne!(first_event.seq, second_event.seq);

    broker
        .wait_run(first.run, WaitOptions::default())
        .await
        .unwrap();
    broker
        .wait_run(second.run, WaitOptions::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn slow_event_consumers_receive_an_explicit_lag_diagnostic() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("output_flood", 0.0))
        .await
        .unwrap();
    let mut events = broker.events(spawned.run).unwrap();
    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    let diagnostic = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .expect("lag diagnostic must be emitted")
        .expect("event stream must remain open");
    match diagnostic.kind {
        RunEventKind::Diagnostic(diagnostic) => {
            assert_eq!(diagnostic.level, DiagnosticLevel::Warning);
            assert!(diagnostic.message.contains("dropped"));
        }
        other => panic!("expected lag diagnostic, got {other:?}"),
    }
}

#[tokio::test]
async fn reasoning_and_tool_lifecycle_are_projected() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(mock_request("rich_events", 0.1))
        .await
        .unwrap();
    let mut events = broker.events(spawned.run).unwrap();

    let first = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .expect("reasoning event projection timed out")
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .expect("tool start projection timed out")
        .unwrap();
    let third = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .unwrap_or_else(|_| panic!("tool completion projection timed out after {second:?}"))
        .unwrap();
    assert!(matches!(first.kind, RunEventKind::ReasoningDelta { .. }));
    assert!(matches!(second.kind, RunEventKind::ToolStarted(_)));
    assert!(matches!(third.kind, RunEventKind::ToolCompleted(_)));
    assert!(first.seq < second.seq && second.seq < third.seq);
    assert_eq!(
        second.provider_meta.as_ref().unwrap()["extensions"][0]["fixture"]["nestedAgentId"],
        "subagent-1"
    );
    assert_eq!(
        second.provider_meta.as_ref().unwrap()["terminal_ids"][0],
        "term-fixture"
    );

    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
}
