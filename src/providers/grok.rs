use std::path::PathBuf;
use std::time::Duration;

use semver::VersionReq;

use super::{AcpVersionPolicy, CapabilitySet, ProviderManifest, VersionProbe};
use crate::ProviderId;

pub const GROK_TESTED_VERSION: &str = "0.2.118";

#[must_use]
pub fn grok_manifest(executable: Option<PathBuf>) -> ProviderManifest {
    ProviderManifest {
        id: ProviderId::Grok,
        command: executable.unwrap_or_else(|| PathBuf::from("grok")),
        args: vec!["--no-auto-update".into(), "agent".into(), "stdio".into()],
        version_probe: VersionProbe::Semver {
            args: vec!["version".into()],
            requirement: VersionReq::parse(&format!("={GROK_TESTED_VERSION}"))
                .expect("tested Grok version must be valid semver"),
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
