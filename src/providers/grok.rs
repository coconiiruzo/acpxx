use std::path::PathBuf;
use std::time::Duration;

use super::{AcpVersionPolicy, ArtifactProbe, CapabilitySet, IdentityProbe, ProviderDriver};
use crate::DriverId;
use crate::ProviderId;

#[must_use]
pub fn grok_driver(executable: Option<PathBuf>) -> ProviderDriver {
    ProviderDriver {
        id: ProviderId::Grok,
        driver_id: DriverId::new("grok-native"),
        driver_revision: 1,
        command: executable.unwrap_or_else(|| PathBuf::from("grok")),
        args: vec!["--no-auto-update".into(), "agent".into(), "stdio".into()],
        identity_probe: IdentityProbe::Semver {
            args: vec!["version".into()],
        },
        artifact_probe: ArtifactProbe::LaunchExecutableSha256 {
            package_metadata: None,
        },
        protocol: AcpVersionPolicy::StableV1,
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
