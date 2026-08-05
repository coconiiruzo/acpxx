use std::path::PathBuf;
use std::time::Duration;

use super::{AcpVersionPolicy, ArtifactProbe, CapabilitySet, IdentityProbe, ProviderDriver};
use crate::{DriverId, ProviderId};

#[must_use]
pub fn cursor_driver(executable: Option<PathBuf>) -> ProviderDriver {
    ProviderDriver {
        id: ProviderId::Cursor,
        driver_id: DriverId::new("cursor-native"),
        driver_revision: 1,
        command: executable.unwrap_or_else(|| PathBuf::from("cursor-agent")),
        args: vec!["acp".into()],
        identity_probe: IdentityProbe::ExactOutput {
            args: vec!["--version".into()],
            strip_prefix: None,
        },
        artifact_probe: ArtifactProbe::LaunchExecutableSha256 {
            package_metadata: None,
        },
        protocol: AcpVersionPolicy::StableV1,
        required_capabilities: CapabilitySet(Vec::new()),
        allowed_env: environment_allowlist(&["CURSOR_API_KEY", "CURSOR_API_ENDPOINT"]),
        fixed_env: Default::default(),
        preferred_auth_method: Some("cursor_login".into()),
        startup_timeout: Duration::from_secs(30),
    }
}

pub(super) fn environment_allowlist(extra: &[&str]) -> Vec<String> {
    [
        "HOME",
        "PATH",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .map(str::to_owned)
    .collect()
}
