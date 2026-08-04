use std::path::PathBuf;
use std::time::Duration;

use acpxx::{
    AcpVersionPolicy, Broker, CapabilitySet, FailureCode, PermissionPolicy, ProviderId,
    ProviderManifest, ProviderSpec, SpawnRequest, Task, TerminalRunState, WaitOptions,
};
use semver::VersionReq;

#[tokio::test]
async fn missing_provider_fails_through_wait_run() {
    let broker = Broker::new(1);
    let spawned = broker
        .spawn(SpawnRequest {
            provider: ProviderSpec::Custom {
                manifest: ProviderManifest {
                    id: ProviderId::new("missing"),
                    command: PathBuf::from("acpxx-provider-that-does-not-exist"),
                    args: Vec::new(),
                    version_args: vec!["--version".into()],
                    expected_version: VersionReq::parse("=1.0.0").unwrap(),
                    protocol: AcpVersionPolicy::StableV1,
                    required_capabilities: CapabilitySet(Vec::new()),
                    allowed_env: Vec::new(),
                    startup_timeout: Duration::from_secs(1),
                },
            },
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            task: Task::new("test"),
            permission_policy: PermissionPolicy::Deny,
        })
        .await
        .unwrap();

    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(receipt.failure.unwrap().code, FailureCode::AdapterNotFound);
    assert_eq!(
        receipt.cleanup.process,
        acpxx::ProcessDisposition::NeverStarted
    );
}

#[tokio::test]
async fn untested_provider_version_is_rejected_before_acp_spawn() {
    let broker = Broker::new(1);
    let mut request = crate::mock_request();
    if let ProviderSpec::Custom { manifest } = &mut request.provider {
        manifest.expected_version = VersionReq::parse("=9.9.9").unwrap();
    }
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();

    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.unwrap().code,
        FailureCode::AdapterVersionMismatch
    );
    assert_eq!(
        receipt.cleanup.process,
        acpxx::ProcessDisposition::NeverStarted
    );
}

fn mock_request() -> SpawnRequest {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
    SpawnRequest {
        provider: ProviderSpec::Custom {
            manifest: ProviderManifest {
                id: ProviderId::new("mock"),
                command: PathBuf::from("python3"),
                args: vec![script.to_string_lossy().into_owned()],
                version_args: vec![script.to_string_lossy().into_owned(), "--version".into()],
                expected_version: VersionReq::parse("=0.1.0").unwrap(),
                protocol: AcpVersionPolicy::StableV1,
                required_capabilities: CapabilitySet(Vec::new()),
                allowed_env: vec!["PATH".into()],
                startup_timeout: Duration::from_secs(2),
            },
        },
        cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        task: Task::new("test"),
        permission_policy: PermissionPolicy::Deny,
    }
}
