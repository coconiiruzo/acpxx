use std::path::PathBuf;
use std::time::Duration;

use super::cursor::environment_allowlist;
use super::{AcpVersionPolicy, CapabilitySet, ProviderManifest, VersionProbe};
use crate::ProviderId;

pub const CLAUDE_ACP_TESTED_VERSION: &str = "0.64.2";
pub const CLAUDE_AGENT_SDK_TESTED_VERSION: &str = "0.3.220";

#[must_use]
pub fn claude_manifest(adapter: Option<PathBuf>) -> ProviderManifest {
    ProviderManifest {
        id: ProviderId::Claude,
        command: adapter.unwrap_or_else(|| PathBuf::from("claude-agent-acp")),
        args: Vec::new(),
        version_probe: VersionProbe::ExactOutput {
            args: vec!["--version".into()],
            expected: CLAUDE_ACP_TESTED_VERSION.into(),
        },
        protocol: AcpVersionPolicy::StableV1,
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
