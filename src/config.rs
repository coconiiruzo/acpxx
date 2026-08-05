use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    CLAUDE_ACP_TESTED_VERSION, CLAUDE_AGENT_SDK_TESTED_VERSION, CODEX_ACP_TESTED_VERSION,
    CODEX_BUNDLED_TESTED_VERSION, CURSOR_TESTED_VERSION, GROK_TESTED_VERSION, PermissionPolicy,
    ProviderId, ProviderSpec,
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(default)]
    pub environment: Option<EnvironmentLock>,
    pub profiles: BTreeMap<String, ProviderProfile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentLock {
    pub platform: String,
    pub acp_protocol: String,
    pub adapter_lockfile: PathBuf,
    pub adapter_lockfile_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderProfile {
    Grok {
        executable: PathBuf,
        args: Vec<String>,
        version: String,
        sha256: String,
        authentication: String,
        initialize_verified: bool,
        session_new_verified: bool,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Cursor {
        executable: PathBuf,
        args: Vec<String>,
        version: String,
        sha256: String,
        authentication: String,
        initialize_verified: bool,
        session_new_verified: bool,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Codex {
        adapter_path: PathBuf,
        adapter_version: String,
        bundled_codex_version: String,
        authentication: String,
        initialize_verified: bool,
        session_new_verified: bool,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Claude {
        adapter_path: PathBuf,
        adapter_version: String,
        claude_agent_sdk_version: String,
        authentication: String,
        initialize_verified: bool,
        session_new_verified: bool,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePermissions {
    #[default]
    Deny,
    AllowAll,
}

#[derive(Clone, Debug)]
pub struct ResolvedProfile {
    pub name: String,
    pub provider: ProviderId,
    pub provider_spec: ProviderSpec,
    pub permission_policy: PermissionPolicy,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read provider config: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid provider config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("provider config exceeds the 1 MiB limit")]
    TooLarge,
    #[error("provider config must be a regular file with mode 0600: {0}")]
    InsecurePermissions(PathBuf),
    #[error("provider profile not found: {0}")]
    ProfileNotFound(String),
    #[error("invalid provider profile {profile}: {message}")]
    InvalidProfile { profile: String, message: String },
}

impl ProviderConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(ConfigError::InsecurePermissions(path.to_owned()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o777 != 0o600 {
                return Err(ConfigError::InsecurePermissions(path.to_owned()));
            }
        }
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge);
        }
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        config.validate_environment()?;
        for name in config.profiles.keys() {
            validate_profile_name(name)?;
        }
        Ok(config)
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedProfile, ConfigError> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| ConfigError::ProfileNotFound(name.to_owned()))?;
        profile.resolve(name)
    }

    fn validate_environment(&self) -> Result<(), ConfigError> {
        let Some(environment) = &self.environment else {
            return Ok(());
        };
        if environment.acp_protocol != "v1" {
            return Err(invalid(
                "environment",
                "only stable ACP protocol v1 is supported",
            ));
        }
        if environment.platform != "macos-arm64" {
            return Err(invalid(
                "environment",
                "v1 configuration platform must be macos-arm64",
            ));
        }
        validate_absolute_file("environment", &environment.adapter_lockfile)?;
        verify_checksum(
            "environment",
            &environment.adapter_lockfile,
            &environment.adapter_lockfile_sha256,
        )
    }
}

impl ProviderProfile {
    fn resolve(&self, name: &str) -> Result<ResolvedProfile, ConfigError> {
        match self {
            Self::Grok {
                executable,
                args,
                version,
                sha256,
                initialize_verified,
                session_new_verified,
                permissions,
                ..
            } => {
                validate_evidence(name, *initialize_verified, *session_new_verified)?;
                validate_exact(name, "version", version, GROK_TESTED_VERSION)?;
                validate_args(name, args, &["--no-auto-update", "agent", "stdio"])?;
                validate_absolute_file(name, executable)?;
                verify_checksum(name, executable, sha256)?;
                Ok(resolved(
                    name,
                    ProviderId::Grok,
                    ProviderSpec::Grok {
                        executable: Some(executable.clone()),
                    },
                    *permissions,
                ))
            }
            Self::Cursor {
                executable,
                args,
                version,
                sha256,
                initialize_verified,
                session_new_verified,
                permissions,
                ..
            } => {
                validate_evidence(name, *initialize_verified, *session_new_verified)?;
                validate_exact(name, "version", version, CURSOR_TESTED_VERSION)?;
                validate_args(name, args, &["acp"])?;
                validate_absolute_file(name, executable)?;
                verify_checksum(name, executable, sha256)?;
                Ok(resolved(
                    name,
                    ProviderId::Cursor,
                    ProviderSpec::Cursor {
                        executable: Some(executable.clone()),
                    },
                    *permissions,
                ))
            }
            Self::Codex {
                adapter_path,
                adapter_version,
                bundled_codex_version,
                initialize_verified,
                session_new_verified,
                permissions,
                ..
            } => {
                validate_evidence(name, *initialize_verified, *session_new_verified)?;
                validate_exact(
                    name,
                    "adapter_version",
                    adapter_version,
                    CODEX_ACP_TESTED_VERSION,
                )?;
                validate_exact(
                    name,
                    "bundled_codex_version",
                    bundled_codex_version,
                    CODEX_BUNDLED_TESTED_VERSION,
                )?;
                validate_absolute_file(name, adapter_path)?;
                Ok(resolved(
                    name,
                    ProviderId::Codex,
                    ProviderSpec::Codex {
                        adapter: Some(adapter_path.clone()),
                    },
                    *permissions,
                ))
            }
            Self::Claude {
                adapter_path,
                adapter_version,
                claude_agent_sdk_version,
                initialize_verified,
                session_new_verified,
                permissions,
                ..
            } => {
                validate_evidence(name, *initialize_verified, *session_new_verified)?;
                validate_exact(
                    name,
                    "adapter_version",
                    adapter_version,
                    CLAUDE_ACP_TESTED_VERSION,
                )?;
                validate_exact(
                    name,
                    "claude_agent_sdk_version",
                    claude_agent_sdk_version,
                    CLAUDE_AGENT_SDK_TESTED_VERSION,
                )?;
                validate_absolute_file(name, adapter_path)?;
                Ok(resolved(
                    name,
                    ProviderId::Claude,
                    ProviderSpec::Claude {
                        adapter: Some(adapter_path.clone()),
                    },
                    *permissions,
                ))
            }
        }
    }
}

fn resolved(
    name: &str,
    provider: ProviderId,
    provider_spec: ProviderSpec,
    permissions: ProfilePermissions,
) -> ResolvedProfile {
    ResolvedProfile {
        name: name.to_owned(),
        provider,
        provider_spec,
        permission_policy: match permissions {
            ProfilePermissions::Deny => PermissionPolicy::Deny,
            ProfilePermissions::AllowAll => PermissionPolicy::AllowAll,
        },
    }
}

fn validate_profile_name(name: &str) -> Result<(), ConfigError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(
            name,
            "profile name contains unsupported characters",
        ));
    }
    Ok(())
}

fn validate_evidence(name: &str, initialize: bool, session: bool) -> Result<(), ConfigError> {
    if !initialize || !session {
        return Err(invalid(
            name,
            "profile has not passed initialize and session/new verification",
        ));
    }
    Ok(())
}

fn validate_exact(
    name: &str,
    field: &str,
    actual: &str,
    expected: &str,
) -> Result<(), ConfigError> {
    if actual != expected {
        return Err(invalid(
            name,
            format!("{field} {actual:?} does not match tested version {expected:?}"),
        ));
    }
    Ok(())
}

fn validate_args(name: &str, actual: &[String], expected: &[&str]) -> Result<(), ConfigError> {
    if !actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        return Err(invalid(
            name,
            "provider arguments do not match the tested manifest",
        ));
    }
    Ok(())
}

fn validate_absolute_file(name: &str, path: &Path) -> Result<(), ConfigError> {
    if !path.is_absolute() {
        return Err(invalid(name, "executable path must be absolute"));
    }
    let metadata = std::fs::metadata(path).map_err(|error| invalid(name, error.to_string()))?;
    if !metadata.is_file() {
        return Err(invalid(name, "executable path is not a file"));
    }
    Ok(())
}

fn verify_checksum(name: &str, path: &Path, expected: &str) -> Result<(), ConfigError> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            name,
            "sha256 must contain exactly 64 hexadecimal digits",
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|error| invalid(name, error.to_string()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| invalid(name, error.to_string()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = format!("{:x}", digest.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(invalid(name, "sha256 does not match the pinned file"));
    }
    Ok(())
}

fn invalid(profile: impl Into<String>, message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidProfile {
        profile: profile.into(),
        message: message.into(),
    }
}

#[must_use]
pub fn default_config_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/agentmux/providers.toml")
}
