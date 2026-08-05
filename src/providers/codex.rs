use std::path::PathBuf;
use std::time::Duration;

use super::cursor::environment_allowlist;
use super::{
    AcpVersionPolicy, ArtifactProbe, CapabilitySet, IdentityProbe, PackageMetadataProbe,
    ProviderDriver,
};
use crate::{DriverId, ProviderId};

#[must_use]
pub fn codex_driver(adapter: Option<PathBuf>) -> ProviderDriver {
    ProviderDriver {
        id: ProviderId::Codex,
        driver_id: DriverId::new("codex-acp"),
        driver_revision: 1,
        command: adapter.unwrap_or_else(|| PathBuf::from("codex-acp")),
        args: Vec::new(),
        identity_probe: IdentityProbe::ExactOutput {
            args: vec!["--version".into()],
            strip_prefix: Some("@agentclientprotocol/codex-acp ".into()),
        },
        artifact_probe: ArtifactProbe::LaunchExecutableSha256 {
            package_metadata: Some(PackageMetadataProbe {
                package_name: "@agentclientprotocol/codex-acp".into(),
                component_dependencies: [("codex".into(), "@openai/codex".into())]
                    .into_iter()
                    .collect(),
            }),
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
    fn driver_routes_mutations_through_acp_permission_requests() {
        let manifest = codex_driver(None);
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
