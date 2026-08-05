use std::path::PathBuf;
use std::time::Duration;

use super::cursor::environment_allowlist;
use super::{
    AcpProtocolPolicy, ArtifactProbe, CapabilitySet, IdentityProbe, PackageMetadataProbe,
    ProviderDriver,
};
use crate::{DriverId, ProviderId};

#[must_use]
pub fn claude_driver(adapter: Option<PathBuf>) -> ProviderDriver {
    ProviderDriver {
        id: ProviderId::Claude,
        driver_id: DriverId::new("claude-agent-acp"),
        driver_revision: 1,
        command: adapter.unwrap_or_else(|| PathBuf::from("claude-agent-acp")),
        args: Vec::new(),
        identity_probe: IdentityProbe::ExactOutput {
            args: vec!["--version".into()],
            strip_prefix: None,
        },
        artifact_probe: ArtifactProbe::LaunchExecutableSha256 {
            package_metadata: Some(PackageMetadataProbe {
                package_name: "@agentclientprotocol/claude-agent-acp".into(),
                component_dependencies: [(
                    "claude_agent_sdk".into(),
                    "@anthropic-ai/claude-agent-sdk".into(),
                )]
                .into_iter()
                .collect(),
            }),
        },
        protocol: AcpProtocolPolicy::StableV1,
        required_capabilities: CapabilitySet(Vec::new()),
        allowed_env: environment_allowlist(&[
            "ANTHROPIC_API_KEY",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CONFIG_DIR",
        ]),
        fixed_env: Default::default(),
        preferred_auth_method: None,
        startup_timeout: Duration::from_secs(30),
    }
}
