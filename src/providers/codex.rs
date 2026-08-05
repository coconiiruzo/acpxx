use std::path::PathBuf;
use std::time::Duration;

use super::cursor::environment_allowlist;
use super::{AcpVersionPolicy, CapabilitySet, ProviderManifest, VersionProbe};
use crate::ProviderId;

pub const CODEX_ACP_TESTED_VERSION: &str = "1.1.9";
pub const CODEX_BUNDLED_TESTED_VERSION: &str = "0.145.0";

#[must_use]
pub fn codex_manifest(adapter: Option<PathBuf>) -> ProviderManifest {
    ProviderManifest {
        id: ProviderId::Codex,
        command: adapter.unwrap_or_else(|| PathBuf::from("codex-acp")),
        args: Vec::new(),
        version_probe: VersionProbe::ExactOutput {
            args: vec!["--version".into()],
            expected: format!("@agentclientprotocol/codex-acp {CODEX_ACP_TESTED_VERSION}"),
        },
        protocol: AcpVersionPolicy::StableV1,
        required_capabilities: CapabilitySet(Vec::new()),
        allowed_env: environment_allowlist(&[
            "CODEX_API_KEY",
            "OPENAI_API_KEY",
            "CODEX_HOME",
            "CODEX_PATH",
            "CODEX_CONFIG",
            "MODEL_PROVIDER",
            "DEFAULT_AUTH_REQUEST",
            "INITIAL_AGENT_MODE",
            "NO_BROWSER",
            "APP_SERVER_LOGS",
        ]),
        fixed_env: [
            ("INITIAL_AGENT_MODE".into(), "read-only".into()),
            (
                "CODEX_CONFIG".into(),
                r#"{"approvals_reviewer":"user","features":{"guardian_approval":false}}"#.into(),
            ),
        ]
        .into_iter()
        .collect(),
        preferred_auth_method: Some("chat-gpt".into()),
        startup_timeout: Duration::from_secs(30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tested_manifest_routes_mutations_through_acp_permission_requests() {
        let manifest = codex_manifest(None);
        assert_eq!(
            manifest
                .fixed_env
                .get("INITIAL_AGENT_MODE")
                .map(String::as_str),
            Some("read-only")
        );
        let config = manifest.fixed_env.get("CODEX_CONFIG").unwrap();
        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(config["approvals_reviewer"], "user");
        assert_eq!(config["features"]["guardian_approval"], false);
        assert!(
            manifest
                .fixed_env
                .keys()
                .all(|name| manifest.allowed_env.contains(name))
        );
    }
}
