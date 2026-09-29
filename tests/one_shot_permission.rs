use std::path::PathBuf;
use std::time::Duration;

use acpxx::acp::run_one_shot;
use acpxx::{PermissionPolicy, RunStage, StopReason, grok_driver};

/// Runs the public one-shot API against the mock agent. The mock exits non-zero, failing the Run,
/// when the permission answer is not the one ACP v1 expects for the policy.
async fn one_shot(mode: &str, policy: PermissionPolicy) -> StopReason {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let driver = grok_driver(Some(manifest_dir.join("tests/fixtures/mock_acp_agent.py")));
    let (stage, _stage_rx) = tokio::sync::watch::channel(RunStage::Admitted);
    let (events, mut events_rx) = tokio::sync::mpsc::channel(64);
    let drain = tokio::spawn(async move { while events_rx.recv().await.is_some() {} });
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        run_one_shot(
            driver,
            manifest_dir,
            format!("return the fixture output __fake_mode={mode} __fake_delay=0"),
            policy,
            stage,
            events,
        ),
    )
    .await
    .expect("one-shot permission callback must not deadlock")
    .unwrap_or_else(|error| panic!("{mode} under {policy:?} failed: {:?}", error.failure));
    drain.abort();
    outcome.stop_reason
}

#[tokio::test]
async fn one_shot_deny_selects_the_reject_once_option() {
    assert_eq!(
        one_shot("permission", PermissionPolicy::Deny).await,
        StopReason::EndTurn
    );
}

#[tokio::test]
async fn one_shot_allow_all_prefers_allow_once_over_a_leading_allow_always() {
    assert_eq!(
        one_shot("permission_allow", PermissionPolicy::AllowAll).await,
        StopReason::EndTurn
    );
}

#[tokio::test]
async fn one_shot_deny_cancels_only_when_no_reject_option_is_advertised() {
    assert_eq!(
        one_shot("permission_without_reject", PermissionPolicy::Deny).await,
        StopReason::EndTurn
    );
}
