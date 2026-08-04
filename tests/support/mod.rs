use std::path::PathBuf;
use std::time::Duration;

use acpxx::{
    AcpVersionPolicy, CapabilitySet, PermissionPolicy, ProviderId, ProviderManifest, ProviderSpec,
    SpawnRequest, Task,
};
use semver::VersionReq;

pub fn mock_request(mode: &str, delay: f64) -> SpawnRequest {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
    SpawnRequest {
        provider: ProviderSpec::Custom {
            manifest: ProviderManifest {
                id: ProviderId::new("mock"),
                command: PathBuf::from("python3"),
                args: vec![
                    script.to_string_lossy().into_owned(),
                    "--mode".into(),
                    mode.into(),
                    "--delay".into(),
                    delay.to_string(),
                ],
                version_args: vec![script.to_string_lossy().into_owned(), "--version".into()],
                expected_version: VersionReq::parse("=0.1.0").unwrap(),
                protocol: AcpVersionPolicy::StableV1,
                required_capabilities: CapabilitySet(Vec::new()),
                allowed_env: vec!["PATH".into(), "LANG".into()],
                startup_timeout: Duration::from_secs(2),
            },
        },
        cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        task: Task::new("return the fixture output"),
        permission_policy: PermissionPolicy::Deny,
    }
}
