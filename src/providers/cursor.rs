use std::path::PathBuf;
use std::time::Duration;

use super::{AcpVersionPolicy, CapabilitySet, ProviderManifest, VersionProbe};
use crate::ProviderId;

pub const CURSOR_TESTED_VERSION: &str = "2026.07.20-8cc9c0b";

#[must_use]
pub fn cursor_manifest(executable: Option<PathBuf>) -> ProviderManifest {
    ProviderManifest {
        id: ProviderId::Cursor,
        command: executable.unwrap_or_else(|| PathBuf::from("cursor-agent")),
        args: vec!["acp".into()],
        version_probe: VersionProbe::ExactOutput {
            args: vec!["--version".into()],
            expected: CURSOR_TESTED_VERSION.into(),
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
