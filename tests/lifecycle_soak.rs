mod support;

use std::time::Duration;

use acpxx::{Broker, FollowupTask, ListQuery, RunState, WaitOptions};
use support::mock_request;

#[tokio::test]
async fn fake_lifecycle_smoke_preserves_invariants_for_one_hundred_runs() {
    fake_lifecycle_soak(100).await;
}

#[tokio::test]
#[ignore = "runs the full Phase 17 fake-provider 10,000-Run lifecycle soak"]
async fn fake_lifecycle_ten_thousand_runs() {
    fake_lifecycle_soak(10_000).await;
}

async fn fake_lifecycle_soak(run_count: usize) {
    let descriptors_before = open_descriptor_count();
    let broker = Broker::new(1);
    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    let first = broker
        .wait_run(
            spawned.run,
            WaitOptions {
                timeout: Some(Duration::from_secs(5)),
            },
        )
        .await
        .unwrap();
    let expected_stamp = first.session_stamp.unwrap();
    let mut parent = spawned.run;

    for sequence in 1..run_count {
        let run = broker
            .followup(
                spawned.agent,
                parent.run_id,
                FollowupTask::new(format!(
                    "soak turn {sequence} __fake_mode=normal __fake_delay=0"
                )),
            )
            .await
            .unwrap();
        let receipt = broker
            .wait_run(
                run,
                WaitOptions {
                    timeout: Some(Duration::from_secs(5)),
                },
            )
            .await
            .unwrap();
        assert_eq!(receipt.session_stamp.as_ref(), Some(&expected_stamp));
        parent = run;
    }

    let snapshot = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(snapshot.agents.len(), 1);
    assert_eq!(snapshot.runs.len(), run_count);
    assert_eq!(snapshot.agents[0].latest_run_id, Some(parent.run_id));
    assert!(
        snapshot
            .runs
            .iter()
            .all(|run| run.state == RunState::Succeeded)
    );

    broker.shutdown().await.unwrap();
    if let (Some(before), Some(after)) = (descriptors_before, open_descriptor_count()) {
        assert!(
            after <= before + 4,
            "file descriptor count grew from {before} to {after}"
        );
    }
}

fn open_descriptor_count() -> Option<usize> {
    std::fs::read_dir("/dev/fd").ok().map(Iterator::count)
}
