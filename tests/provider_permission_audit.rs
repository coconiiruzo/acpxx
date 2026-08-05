mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use acpxx::{
    Broker, PermissionPolicy, ProviderId, RunEvent, RunEventKind, SpawnRequest, Task, WaitOptions,
};
use futures::StreamExt;
use support::pinned_provider_spec;
use uuid::Uuid;

#[tokio::test]
#[ignore = "executes an authenticated Codex approval allow/deny audit"]
async fn codex_real_approval_allow_and_deny() {
    permission_audit(
        ProviderId::Codex,
        "AGENTMUX_CODEX_ACP_PATH",
        "codex-default",
    )
    .await;
}

#[tokio::test]
#[ignore = "executes an authenticated Claude permission allow/deny audit"]
async fn claude_real_permission_allow_and_deny() {
    permission_audit(
        ProviderId::Claude,
        "AGENTMUX_CLAUDE_ACP_PATH",
        "claude-default",
    )
    .await;
}

async fn permission_audit(provider: ProviderId, variable: &str, profile: &str) {
    let fixture = PermissionFixture::new(provider);
    let denied = fixture.target_directory.join("denied.txt");
    let deny_event = run_write_attempt(
        provider,
        variable,
        profile,
        &fixture.workspace,
        &denied,
        PermissionPolicy::Deny,
    )
    .await;
    assert!(!denied.exists(), "{provider} mutated the denied path");
    assert!(matches!(
        deny_event.kind,
        RunEventKind::PermissionRequested(_)
    ));
    assert_provider_permission_metadata(provider, &deny_event);

    let allowed = fixture.target_directory.join("allowed.txt");
    let allow_event = run_write_attempt(
        provider,
        variable,
        profile,
        &fixture.workspace,
        &allowed,
        PermissionPolicy::AllowAll,
    )
    .await;
    assert!(matches!(
        allow_event.kind,
        RunEventKind::PermissionRequested(_)
    ));
    assert_provider_permission_metadata(provider, &allow_event);
    assert_eq!(
        std::fs::read_to_string(&allowed).unwrap(),
        "agentmux-permission-ok"
    );
}

fn assert_provider_permission_metadata(provider: ProviderId, event: &RunEvent) {
    let expected_key = match provider {
        ProviderId::Codex => "codex",
        ProviderId::Claude => "permission",
        ProviderId::Grok | ProviderId::Cursor => return,
    };
    let extensions = event
        .provider_meta
        .as_ref()
        .and_then(|metadata| metadata.get("extensions"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{provider} permission event omitted provider metadata"));
    assert!(
        extensions
            .iter()
            .any(|metadata| metadata.get(expected_key).is_some()),
        "{provider} permission metadata omitted {expected_key}: {:?}",
        event.provider_meta
    );
}

async fn run_write_attempt(
    provider: ProviderId,
    variable: &str,
    profile: &str,
    cwd: &Path,
    target: &Path,
    policy: PermissionPolicy,
) -> RunEvent {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(SpawnRequest {
            provider: pinned_provider_spec(provider, variable, profile),
            cwd: cwd.to_owned(),
            task: Task::new(format!(
                "Use your normal shell or file tool to write exactly agentmux-permission-ok to {}. Do not merely describe the command.",
                target.display()
            ))
            .with_deadline(Duration::from_secs(120)),
            permission_policy: policy,
        })
        .await
        .unwrap();
    let mut events = broker.events(spawned.run).unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = observed.clone();
    let permission = async {
        while let Some(event) = events.next().await {
            event_log.lock().unwrap().push(format!("{:?}", event.kind));
            if matches!(event.kind, RunEventKind::PermissionRequested(_)) {
                return event;
            }
        }
        panic!("{provider} event stream closed before permission")
    };
    let mut receipt = Box::pin(broker.wait_run(
        spawned.run,
        WaitOptions {
            timeout: Some(Duration::from_secs(130)),
        },
    ));
    let mut permission = Box::pin(tokio::time::timeout(Duration::from_secs(120), permission));
    let permission = tokio::select! {
        result = &mut permission => {
            result.unwrap_or_else(|_| {
                panic!(
                    "{provider} did not request permission under {policy:?}; events={:?}",
                    observed.lock().unwrap()
                )
            })
        }
        result = &mut receipt => {
            let receipt = result.unwrap();
            panic!(
                "{provider} reached terminal without requesting permission under {policy:?}; receipt={receipt:?}; events={:?}",
                observed.lock().unwrap()
            )
        }
    };
    let receipt = receipt.await.unwrap();
    assert!(
        receipt.failure.is_none() || policy == PermissionPolicy::Deny,
        "{provider} {policy:?}: {receipt:?}"
    );
    broker.shutdown().await.unwrap();
    permission
}

struct PermissionFixture {
    workspace: PathBuf,
    target_directory: PathBuf,
}

impl PermissionFixture {
    fn new(provider: ProviderId) -> Self {
        let id = Uuid::now_v7().as_simple().to_string();
        let workspace = PathBuf::from("/tmp").join(format!(
            "amx-perm-{}-{}",
            provider.as_str(),
            &id[id.len() - 8..]
        ));
        let target_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/provider-permission-audit")
            .join(id);
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&target_directory).unwrap();
        Self {
            workspace,
            target_directory,
        }
    }
}

impl Drop for PermissionFixture {
    fn drop(&mut self) {
        for name in ["denied.txt", "allowed.txt"] {
            let _ = std::fs::remove_file(self.target_directory.join(name));
        }
        let _ = std::fs::remove_dir(&self.workspace);
        let _ = std::fs::remove_dir(&self.target_directory);
    }
}
