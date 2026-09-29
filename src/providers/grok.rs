use std::path::PathBuf;
use std::time::Duration;

use super::{AcpProtocolPolicy, ArtifactProbe, CapabilitySet, IdentityProbe, ProviderDriver};
use crate::{DriverId, ProviderId};

#[must_use]
pub fn grok_driver(executable: Option<PathBuf>) -> ProviderDriver {
    ProviderDriver {
        id: ProviderId::Grok,
        driver_id: DriverId::new("grok-native"),
        driver_revision: 2,
        command: executable.unwrap_or_else(|| PathBuf::from("grok")),
        // `--permission-mode default` overrides a user or project always-approve default so
        // mutations reach agentmux as ACP permission requests. `--no-leader` keeps tools inside
        // the owned process instead of a shared leader started outside agentmux.
        args: vec![
            "--no-auto-update".into(),
            "--permission-mode".into(),
            "default".into(),
            "agent".into(),
            "--no-leader".into(),
            "stdio".into(),
        ],
        identity_probe: IdentityProbe::Semver {
            args: vec!["version".into()],
        },
        artifact_probe: ArtifactProbe::LaunchExecutableSha256 {
            package_metadata: None,
        },
        protocol: AcpProtocolPolicy::StableV1,
        required_capabilities: CapabilitySet(Vec::new()),
        allowed_env: [
            "HOME",
            "PATH",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "XAI_API_KEY",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "NO_PROXY",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        fixed_env: Default::default(),
        preferred_auth_method: Some("cached_token".into()),
        startup_timeout: Duration::from_secs(30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_forces_ask_permission_mode_and_a_local_agent() {
        let driver = grok_driver(None);
        assert_eq!(
            driver.args,
            [
                "--no-auto-update",
                "--permission-mode",
                "default",
                "agent",
                "--no-leader",
                "stdio",
            ]
        );
        assert_eq!(driver.driver_revision, 2);
    }
}
