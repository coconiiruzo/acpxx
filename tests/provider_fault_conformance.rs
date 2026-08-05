#![cfg(unix)]

mod support;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use acpxx::{
    AdmissionError, Broker, Continuity, ContinuityLossReason, ControlError, FailureCode,
    FollowupTask, PermissionPolicy, ProcessDisposition, ProviderId, RunEventKind, Task,
    TerminalRunState, WaitOptions,
};
use futures::StreamExt;
use support::MockProviderFixture;
use uuid::Uuid;

const PROVIDERS: [ProviderId; 4] = [
    ProviderId::Grok,
    ProviderId::Cursor,
    ProviderId::Codex,
    ProviderId::Claude,
];

#[tokio::test]
async fn every_provider_profile_classifies_crash_and_malformed_protocol_without_fallback() {
    for provider in PROVIDERS {
        assert_fault(provider, "crash", FailureCode::ProviderCrashed).await;
        assert_fault(provider, "malformed", FailureCode::ProtocolCorruption).await;
    }
}

#[tokio::test]
async fn every_provider_profile_enforces_permission_deny_and_explicit_allow() {
    for provider in PROVIDERS {
        for (mode, policy) in [
            ("permission", PermissionPolicy::Deny),
            ("permission_allow", PermissionPolicy::AllowAll),
        ] {
            let fixture = MockProviderFixture::new(provider);
            let broker = Broker::new(1);
            let mut request = fixture.request(mode);
            request.permission_policy = policy;
            request.allow_unverified_mutations = policy == PermissionPolicy::AllowAll;
            let spawned = broker.spawn(request).await.unwrap();
            let mut events = broker.events(spawned.run).unwrap();
            let permission = tokio::time::timeout(Duration::from_secs(2), async {
                while let Some(event) = events.next().await {
                    if matches!(event.kind, RunEventKind::PermissionRequested(_)) {
                        return event;
                    }
                }
                panic!("permission event stream closed")
            });
            let (receipt, permission) = tokio::join!(
                broker.wait_run(spawned.run, WaitOptions::default()),
                permission
            );
            let permission = permission.expect("permission callback was not projected");
            let metadata = permission
                .provider_meta
                .expect("permission provider metadata was not projected");
            assert_eq!(metadata["extensions"][0]["fixture"]["event"], "approval");
            assert_eq!(metadata["extensions"][0]["fixture"]["secret"], "[REDACTED]");
            assert_eq!(
                receipt.unwrap().state,
                TerminalRunState::Succeeded,
                "{provider} {policy:?}"
            );
            broker.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn every_provider_profile_reaps_owned_descendants_on_shutdown() {
    for provider in PROVIDERS {
        let fixture = MockProviderFixture::new(provider);
        let pid_file = std::env::temp_dir().join(format!(
            "agentmux-{}-grandchild-{}.pid",
            provider.as_str(),
            Uuid::now_v7()
        ));
        let mut request = fixture.request("grandchild");
        request.task = Task::new(format!(
            "controlled conformance __fake_mode=grandchild __fake_pid_file={}",
            pid_file.display()
        ));
        let broker = Broker::new(1);
        let spawned = broker.spawn(request).await.unwrap();
        let receipt = broker
            .wait_run(spawned.run, WaitOptions::default())
            .await
            .unwrap();
        assert_eq!(receipt.state, TerminalRunState::Succeeded, "{provider}");
        assert_eq!(receipt.cleanup.process, ProcessDisposition::Retained);
        let pid = read_pid(&pid_file).await;

        broker.shutdown().await.unwrap();
        assert_process_dies(pid).await;
        let _ = std::fs::remove_file(pid_file);
    }
}

async fn assert_fault(provider: ProviderId, mode: &str, expected: FailureCode) {
    let fixture = MockProviderFixture::new(provider);
    let broker = Broker::new(1);
    let spawned = broker.spawn(fixture.request(mode)).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Failed, "{provider} {mode}");
    assert_eq!(
        receipt.failure.as_ref().map(|failure| failure.code),
        Some(expected),
        "{provider} {mode}: {receipt:?}"
    );
    assert_eq!(receipt.cleanup.process, ProcessDisposition::Terminated);
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
    let snapshot = broker.list(Default::default()).await.unwrap();
    let expected_loss = if expected == FailureCode::ProtocolCorruption {
        ContinuityLossReason::ProtocolCorruption
    } else {
        ContinuityLossReason::ProviderExited
    };
    assert_eq!(
        snapshot.agents[0].continuity,
        Some(Continuity::Lost(expected_loss))
    );
    broker.shutdown().await.unwrap();
}

async fn read_pid(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(path) {
                return value.parse().unwrap();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fixture did not publish descendant PID")
}

async fn assert_process_dies(pid: u32) {
    for _ in 0..100 {
        if !process_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if process_alive(pid) {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stderr(Stdio::null())
            .status();
        panic!("owned descendant {pid} survived provider shutdown");
    }
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
