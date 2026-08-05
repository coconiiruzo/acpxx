mod support;

use std::path::PathBuf;
use std::time::Duration;

use acpxx::{
    Broker, Continuity, ContinuityLossReason, FollowupTask, ListQuery, PermissionPolicy,
    ProviderId, RunEventKind, SpawnRequest, Task, TerminalRunState, WaitOptions,
};
use futures::StreamExt;

use support::pinned_provider_spec;

#[tokio::test]
#[ignore = "requires authenticated Grok CLI and may consume model quota"]
async fn grok_authenticated_conformance() {
    authenticated_conformance(ProviderId::Grok, "AGENTMUX_GROK_PATH", "grok-default").await;
}

#[tokio::test]
#[ignore = "requires authenticated Cursor Agent and may consume model quota"]
async fn cursor_authenticated_conformance() {
    authenticated_conformance(ProviderId::Cursor, "AGENTMUX_CURSOR_PATH", "cursor-default").await;
}

#[tokio::test]
#[ignore = "requires authenticated Codex ACP adapter and may consume model quota"]
async fn codex_authenticated_conformance() {
    authenticated_conformance(
        ProviderId::Codex,
        "AGENTMUX_CODEX_ACP_PATH",
        "codex-default",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires authenticated Claude ACP adapter and may consume model quota"]
async fn claude_authenticated_conformance() {
    authenticated_conformance(
        ProviderId::Claude,
        "AGENTMUX_CLAUDE_ACP_PATH",
        "claude-default",
    )
    .await;
}

async fn authenticated_conformance(provider: ProviderId, path_variable: &str, profile: &str) {
    let provider_spec = pinned_provider_spec(provider, path_variable, profile);
    let broker = Broker::new(1);
    let marker = format!("agentmux-{}-first-ok", provider.as_str());
    let spawned = broker
        .spawn(SpawnRequest {
            provider: provider_spec,
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            task: Task::new(format!(
                "Do not use tools. Reply with exactly this text: {marker}"
            ))
            .with_deadline(Duration::from_secs(120)),
            permission_policy: PermissionPolicy::Deny,
            version_policy: acpxx::VersionPolicy::Verified,
            catalog_entry: None,
            allow_unverified_mutations: false,
        })
        .await
        .unwrap();
    let events = broker.events(spawned.run).unwrap();
    let (first, streamed) = tokio::join!(
        broker.wait_run(
            spawned.run,
            WaitOptions {
                timeout: Some(Duration::from_secs(130)),
            },
        ),
        observe_first_output(events),
    );
    let first = first.unwrap();
    assert_eq!(first.state, TerminalRunState::Succeeded, "{first:?}");
    assert!(first.output.text.contains(&marker), "{first:?}");
    assert!(
        streamed
            .iter()
            .any(|event| matches!(event.kind, RunEventKind::OutputDelta { .. })),
        "provider produced no streaming output: {streamed:?}"
    );
    assert!(
        streamed
            .windows(2)
            .all(|events| events[0].seq < events[1].seq),
        "provider stream sequence is not strictly increasing: {streamed:?}"
    );
    let first_stamp = first.session_stamp.clone().expect("missing session stamp");
    let initialized = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(initialized.agents[0].provider_capabilities.is_some());

    let followup_marker = format!("agentmux-{}-followup-ok", provider.as_str());
    let followup = broker
        .followup(
            spawned.agent,
            spawned.run.run_id,
            FollowupTask::new(format!(
                "Do not use tools. Reply with exactly this text: {followup_marker}"
            ))
            .with_deadline(Duration::from_secs(120)),
        )
        .await
        .unwrap();
    let second = broker
        .wait_run(
            followup,
            WaitOptions {
                timeout: Some(Duration::from_secs(130)),
            },
        )
        .await
        .unwrap();
    assert_eq!(second.state, TerminalRunState::Succeeded, "{second:?}");
    assert!(second.output.text.contains(&followup_marker), "{second:?}");
    assert!(
        !second.output.text.contains(&marker),
        "previous Run output was duplicated into the follow-up: {second:?}"
    );
    assert_eq!(second.session_stamp.as_ref(), Some(&first_stamp));
    assert_eq!(second.provider_lock, first.provider_lock);

    let cancellable = broker
        .followup(
            spawned.agent,
            followup.run_id,
            FollowupTask::new(
                "Do not use tools. Produce a list containing ten thousand numbered lines.",
            )
            .with_deadline(Duration::from_secs(120)),
        )
        .await
        .unwrap();
    let cancellable_events = broker.events(cancellable).unwrap();
    let streamed_before_cancel = observe_first_output(cancellable_events).await;
    assert!(
        streamed_before_cancel
            .iter()
            .any(|event| matches!(event.kind, RunEventKind::OutputDelta { .. }))
    );
    broker.interrupt(cancellable).await.unwrap();
    let cancelled = broker
        .wait_run(
            cancellable,
            WaitOptions {
                timeout: Some(Duration::from_secs(30)),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        cancelled.state,
        TerminalRunState::Interrupted,
        "{cancelled:?}"
    );
    let agent = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        agent.agents[0].continuity,
        Some(Continuity::Available(_))
    ));

    let recovery_marker = format!("agentmux-{}-cancel-recovery-ok", provider.as_str());
    let recovery = broker
        .followup(
            spawned.agent,
            cancellable.run_id,
            FollowupTask::new(format!(
                "Do not use tools. Reply with exactly this text: {recovery_marker}"
            ))
            .with_deadline(Duration::from_secs(120)),
        )
        .await
        .unwrap();
    let recovery = broker
        .wait_run(
            recovery,
            WaitOptions {
                timeout: Some(Duration::from_secs(130)),
            },
        )
        .await
        .unwrap();
    assert_eq!(recovery.state, TerminalRunState::Succeeded, "{recovery:?}");
    assert!(
        recovery.output.text.contains(&recovery_marker),
        "{recovery:?}"
    );
    assert_eq!(recovery.session_stamp.as_ref(), Some(&first_stamp));
    broker.shutdown().await.unwrap();
    let stopped = broker
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert!(!stopped.agents[0].process_alive);
    assert!(matches!(
        stopped.agents[0].continuity,
        Some(Continuity::Lost(ContinuityLossReason::HostShutdown))
    ));
}

async fn observe_first_output(mut events: acpxx::runtime::RunEventStream) -> Vec<acpxx::RunEvent> {
    tokio::time::timeout(Duration::from_secs(130), async {
        let mut observed = Vec::new();
        while let Some(event) = events.next().await {
            let is_output = matches!(event.kind, RunEventKind::OutputDelta { .. });
            observed.push(event);
            if is_output {
                return observed;
            }
        }
        panic!("Run event stream closed before output")
    })
    .await
    .expect("provider did not stream output before the conformance deadline")
}
