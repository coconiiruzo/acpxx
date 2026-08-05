mod support;

use std::time::Duration;

use acpxx::{
    AdmissionError, AgentMessage, Broker, Continuity, ContinuityLossReason, ControlError,
    FollowupTask, ListQuery, TerminalRunState, WaitOptions,
};
use support::mock_request;

#[tokio::test]
async fn mailbox_and_three_followups_preserve_the_session_stamp() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    let first = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    let initial_stamp = current_stamp(&broker, spawned.agent.agent_id).await;

    let first_message = broker
        .send(
            spawned.agent,
            AgentMessage {
                content: "mailbox A".into(),
            },
        )
        .await
        .unwrap();
    let second_message = broker
        .send(
            spawned.agent,
            AgentMessage {
                content: "mailbox B".into(),
            },
        )
        .await
        .unwrap();
    assert!(first_message.accepted_sequence < second_message.accepted_sequence);

    let mut parent = first.run_id;
    for turn in 0..3 {
        let run = broker
            .followup(
                spawned.agent,
                parent,
                FollowupTask::new(format!("follow-up {turn}")),
            )
            .await
            .unwrap();
        let receipt = broker.wait_run(run, WaitOptions::default()).await.unwrap();
        assert_eq!(receipt.state, TerminalRunState::Succeeded);
        assert_eq!(receipt.parent_run_id, Some(parent));
        assert_eq!(
            current_stamp(&broker, spawned.agent.agent_id).await,
            initial_stamp
        );
        parent = run.run_id;
    }

    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.agents[0].mailbox_depth, 0);
}

#[tokio::test]
async fn stale_parent_and_lost_continuity_are_admission_errors() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    let next = broker
        .followup(spawned.agent, spawned.run.run_id, FollowupTask::new("next"))
        .await
        .unwrap();
    broker.wait_run(next, WaitOptions::default()).await.unwrap();
    let stale = broker
        .followup(
            spawned.agent,
            spawned.run.run_id,
            FollowupTask::new("stale"),
        )
        .await;
    assert!(matches!(
        stale,
        Err(ControlError::Admission(AdmissionError::StaleParent(_)))
    ));

    let crashed = broker.spawn(mock_request("crash", 0.0)).await.unwrap();
    broker
        .wait_run(crashed.run, WaitOptions::default())
        .await
        .unwrap();
    let lost = broker
        .followup(
            crashed.agent,
            crashed.run.run_id,
            FollowupTask::new("must not create a session"),
        )
        .await;
    assert!(matches!(
        lost,
        Err(ControlError::Admission(
            AdmissionError::ContinuityAlreadyLost(_)
        ))
    ));
}

#[tokio::test]
async fn followup_cutoff_leaves_later_messages_for_the_next_run() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    broker
        .send(
            spawned.agent,
            AgentMessage {
                content: "belongs to first follow-up".into(),
            },
        )
        .await
        .unwrap();
    let first = broker
        .followup(
            spawned.agent,
            spawned.run.run_id,
            FollowupTask::new("first"),
        )
        .await
        .unwrap();
    broker
        .send(
            spawned.agent,
            AgentMessage {
                content: "belongs to second follow-up".into(),
            },
        )
        .await
        .unwrap();
    broker
        .wait_run(first, WaitOptions::default())
        .await
        .unwrap();
    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.agents[0].mailbox_depth, 1);

    let second = broker
        .followup(spawned.agent, first.run_id, FollowupTask::new("second"))
        .await
        .unwrap();
    broker
        .wait_run(second, WaitOptions::default())
        .await
        .unwrap();
    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.agents[0].mailbox_depth, 0);
}

#[tokio::test]
async fn concurrent_followups_admit_only_one_run() {
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    let first_broker = broker.clone();
    let second_broker = broker.clone();
    let first = tokio::spawn(async move {
        first_broker
            .followup(
                spawned.agent,
                spawned.run.run_id,
                FollowupTask::new("candidate one"),
            )
            .await
    });
    let second = tokio::spawn(async move {
        second_broker
            .followup(
                spawned.agent,
                spawned.run.run_id,
                FollowupTask::new("candidate two"),
            )
            .await
    });
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let accepted = results.into_iter().find_map(Result::ok).unwrap();
    broker
        .wait_run(accepted, WaitOptions::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn idle_ttl_ends_the_live_session_without_creating_a_replacement() {
    let broker = Broker::with_idle_ttl(1, Duration::from_millis(50));
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = broker
                .list(ListQuery {
                    agent_id: Some(spawned.agent.agent_id),
                    ..ListQuery::default()
                })
                .await
                .unwrap();
            if matches!(
                snapshot.agents[0].continuity,
                Some(Continuity::Lost(ContinuityLossReason::IdleExpired))
            ) {
                assert!(!snapshot.agents[0].process_alive);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Agent must expire after its idle TTL");

    let followup = broker
        .followup(
            spawned.agent,
            spawned.run.run_id,
            FollowupTask::new("must not create a replacement session"),
        )
        .await;
    assert!(matches!(
        followup,
        Err(ControlError::Admission(
            AdmissionError::ContinuityAlreadyLost(_)
        ))
    ));
}

#[tokio::test]
async fn send_extends_idle_ttl_without_starting_a_run() {
    let broker = Broker::with_idle_ttl(1, Duration::from_millis(200));
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(75)).await;
    broker
        .send(
            spawned.agent,
            AgentMessage {
                content: "extend idle lease".into(),
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        snapshot.agents[0].continuity,
        Some(Continuity::Available(_))
    ));
    assert_eq!(snapshot.agents[0].latest_run_id, Some(spawned.run.run_id));
    assert_eq!(snapshot.agents[0].mailbox_depth, 1);
}

async fn current_stamp(broker: &Broker, agent_id: acpxx::AgentId) -> acpxx::SessionStamp {
    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    match snapshot.agents[0].continuity.clone() {
        Some(Continuity::Available(stamp)) => stamp,
        other => panic!("expected available continuity, got {other:?}"),
    }
}
