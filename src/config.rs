use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ArtifactDigest, CatalogEntry, CompatibilityCatalog, DigestAlgorithm, PermissionPolicy,
    ProviderId, ProviderIdentity, ProviderSpec, VersionPolicy,
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub const CONFIG_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub schema_version: u32,
    pub catalog: CatalogConfig,
    pub profiles: BTreeMap<String, ProviderProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogConfig {
    pub source: CatalogSourceConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSourceConfig {
    Official,
    File,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderProfile {
    Grok {
        executable: PathBuf,
        #[serde(default)]
        version_policy: VersionPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Cursor {
        executable: PathBuf,
        #[serde(default)]
        version_policy: VersionPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Codex {
        adapter_path: PathBuf,
        #[serde(default)]
        version_policy: VersionPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Claude {
        adapter_path: PathBuf,
        #[serde(default)]
        version_policy: VersionPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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
    pub version_policy: VersionPolicy,
    pub catalog_entry: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ConfigMigration {
    pub rendered: Option<String>,
    pub blocking_issues: Vec<String>,
    pub warnings: Vec<String>,
}

impl ConfigMigration {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.blocking_issues.is_empty() && self.rendered.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read provider config: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid provider config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("failed to serialize provider config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("provider config exceeds the 1 MiB limit")]
    TooLarge,
    #[error("provider config must be a regular file with mode 0600: {0}")]
    InsecurePermissions(PathBuf),
    #[error("provider config schema {found:?} requires `agentmux config migrate --check`")]
    MigrationRequired { found: Option<u32> },
    #[error("unsupported provider config schema {0}")]
    UnsupportedSchema(u32),
    #[error("provider profile not found: {0}")]
    ProfileNotFound(String),
    #[error("invalid provider profile {profile}: {message}")]
    InvalidProfile { profile: String, message: String },
    #[error("provider config migration is blocked: {0}")]
    MigrationBlocked(String),
}

impl ProviderConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = read_secure_config(path.as_ref())?;
        let value: toml::Value = toml::from_str(&content)?;
        let schema = value
            .get("schema_version")
            .and_then(toml::Value::as_integer)
            .and_then(|value| u32::try_from(value).ok());
        match schema {
            None | Some(1) => return Err(ConfigError::MigrationRequired { found: schema }),
            Some(CONFIG_SCHEMA_VERSION) => {}
            Some(other) => return Err(ConfigError::UnsupportedSchema(other)),
        }
        let config: Self = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedProfile, ConfigError> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| ConfigError::ProfileNotFound(name.to_owned()))?;
        profile.resolve(name)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        match self.catalog.source {
            CatalogSourceConfig::Official => {
                if self.catalog.path.is_some() || self.catalog.signature_path.is_some() {
                    return Err(invalid(
                        "catalog",
                        "official source cannot specify path or signature_path",
                    ));
                }
            }
            CatalogSourceConfig::File => {
                let path = self
                    .catalog
                    .path
                    .as_deref()
                    .ok_or_else(|| invalid("catalog", "file source requires path"))?;
                let signature = self
                    .catalog
                    .signature_path
                    .as_deref()
                    .ok_or_else(|| invalid("catalog", "file source requires signature_path"))?;
                validate_absolute_file("catalog", path)?;
                validate_absolute_file("catalog", signature)?;
            }
        }
        for (name, profile) in &self.profiles {
            validate_profile_name(name)?;
            profile.validate(name)?;
        }
        Ok(())
    }
}

impl ProviderProfile {
    fn validate(&self, name: &str) -> Result<(), ConfigError> {
        let (path, policy, entry) = match self {
            Self::Grok {
                executable,
                version_policy,
                catalog_entry,
                ..
            }
            | Self::Cursor {
                executable,
                version_policy,
                catalog_entry,
                ..
            } => (executable, version_policy, catalog_entry),
            Self::Codex {
                adapter_path,
                version_policy,
                catalog_entry,
                ..
            }
            | Self::Claude {
                adapter_path,
                version_policy,
                catalog_entry,
                ..
            } => (adapter_path, version_policy, catalog_entry),
        };
        validate_absolute_file(name, path)?;
        if *policy == VersionPolicy::Exact && entry.is_none() {
            return Err(invalid(name, "exact version_policy requires catalog_entry"));
        }
        if *policy != VersionPolicy::Exact && entry.is_some() {
            return Err(invalid(
                name,
                "catalog_entry is only valid with exact version_policy",
            ));
        }
        Ok(())
    }

    fn resolve(&self, name: &str) -> Result<ResolvedProfile, ConfigError> {
        self.validate(name)?;
        let (provider, provider_spec, version_policy, catalog_entry, permissions) = match self {
            Self::Grok {
                executable,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Grok,
                ProviderSpec::Grok {
                    executable: Some(executable.clone()),
                },
                *version_policy,
                catalog_entry.clone(),
                *permissions,
            ),
            Self::Cursor {
                executable,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Cursor,
                ProviderSpec::Cursor {
                    executable: Some(executable.clone()),
                },
                *version_policy,
                catalog_entry.clone(),
                *permissions,
            ),
            Self::Codex {
                adapter_path,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Codex,
                ProviderSpec::Codex {
                    adapter: Some(adapter_path.clone()),
                },
                *version_policy,
                catalog_entry.clone(),
                *permissions,
            ),
            Self::Claude {
                adapter_path,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Claude,
                ProviderSpec::Claude {
                    adapter: Some(adapter_path.clone()),
                },
                *version_policy,
                catalog_entry.clone(),
                *permissions,
            ),
        };
        Ok(ResolvedProfile {
            name: name.to_owned(),
            provider,
            provider_spec,
            permission_policy: match permissions {
                ProfilePermissions::Deny => PermissionPolicy::Deny,
                ProfilePermissions::AllowAll => PermissionPolicy::AllowAll,
            },
            version_policy,
            catalog_entry,
        })
    }
}

pub fn check_migration(
    path: impl AsRef<Path>,
    catalog: &CompatibilityCatalog,
) -> Result<ConfigMigration, ConfigError> {
    let content = read_secure_config(path.as_ref())?;
    let legacy: LegacyProviderConfig = toml::from_str(&content)?;
    migrate_legacy(legacy, catalog)
}

pub fn write_migration(
    path: impl AsRef<Path>,
    catalog: &CompatibilityCatalog,
) -> Result<PathBuf, ConfigError> {
    let path = path.as_ref();
    let migration = check_migration(path, catalog)?;
    if !migration.blocking_issues.is_empty() {
        return Err(ConfigError::MigrationBlocked(
            migration.blocking_issues.join("; "),
        ));
    }
    let rendered = migration
        .rendered
        .ok_or_else(|| ConfigError::MigrationBlocked("migration produced no config".into()))?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup = path.with_extension(format!("toml.v1-backup-{timestamp}"));
    let original = std::fs::read(path)?;
    write_new_private(&backup, &original)?;
    atomic_replace_private(path, rendered.as_bytes())?;
    Ok(backup)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProviderConfig {
    #[serde(default)]
    environment: Option<LegacyEnvironmentLock>,
    profiles: BTreeMap<String, LegacyProviderProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyEnvironmentLock {
    platform: String,
    acp_protocol: String,
    adapter_lockfile: PathBuf,
    adapter_lockfile_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
enum LegacyProviderProfile {
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

fn migrate_legacy(
    legacy: LegacyProviderConfig,
    catalog: &CompatibilityCatalog,
) -> Result<ConfigMigration, ConfigError> {
    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    if let Some(environment) = legacy.environment {
        if environment.platform != "macos-arm64" || environment.acp_protocol != "v1" {
            issues.push("legacy environment is not macos-arm64 / ACP v1".into());
        }
        if let Err(error) = verify_checksum(
            "environment",
            &environment.adapter_lockfile,
            &environment.adapter_lockfile_sha256,
        ) {
            issues.push(error.to_string());
        }
        warnings.push("legacy environment lock is replaced by Driver and Catalog trust".into());
    }
    let mut profiles = BTreeMap::new();
    for (name, profile) in legacy.profiles {
        validate_profile_name(&name)?;
        match migrate_profile(&name, profile, catalog) {
            Ok((profile, mut profile_warnings)) => {
                profiles.insert(name, profile);
                warnings.append(&mut profile_warnings);
            }
            Err(error) => issues.push(error.to_string()),
        }
    }
    let rendered = if issues.is_empty() {
        Some(toml::to_string_pretty(&ProviderConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            catalog: CatalogConfig {
                source: CatalogSourceConfig::Official,
                path: None,
                signature_path: None,
            },
            profiles,
        })?)
    } else {
        None
    };
    Ok(ConfigMigration {
        rendered,
        blocking_issues: issues,
        warnings,
    })
}

fn migrate_profile(
    name: &str,
    profile: LegacyProviderProfile,
    catalog: &CompatibilityCatalog,
) -> Result<(ProviderProfile, Vec<String>), ConfigError> {
    let (provider, path, identity, artifacts, permissions, args_valid, evidence) = match profile {
        LegacyProviderProfile::Grok {
            executable,
            args,
            version,
            sha256,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => (
            ProviderId::Grok,
            executable,
            identity(version, None),
            vec![artifact("executable", sha256)],
            permissions,
            args.iter()
                .map(String::as_str)
                .eq(["--no-auto-update", "agent", "stdio"]),
            (authentication, initialize_verified, session_new_verified),
        ),
        LegacyProviderProfile::Cursor {
            executable,
            args,
            version,
            sha256,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => (
            ProviderId::Cursor,
            executable,
            identity(version, None),
            vec![artifact("executable", sha256)],
            permissions,
            args.iter().map(String::as_str).eq(["acp"]),
            (authentication, initialize_verified, session_new_verified),
        ),
        LegacyProviderProfile::Codex {
            adapter_path,
            adapter_version,
            bundled_codex_version,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => {
            let (artifacts, _) = adapter_artifacts(name, &adapter_path)?;
            (
                ProviderId::Codex,
                adapter_path,
                identity(adapter_version, Some(("codex", bundled_codex_version))),
                artifacts,
                permissions,
                true,
                (authentication, initialize_verified, session_new_verified),
            )
        }
        LegacyProviderProfile::Claude {
            adapter_path,
            adapter_version,
            claude_agent_sdk_version,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => {
            let (artifacts, _) = adapter_artifacts(name, &adapter_path)?;
            (
                ProviderId::Claude,
                adapter_path,
                identity(
                    adapter_version,
                    Some(("claude_agent_sdk", claude_agent_sdk_version)),
                ),
                artifacts,
                permissions,
                true,
                (authentication, initialize_verified, session_new_verified),
            )
        }
    };
    validate_absolute_file(name, &path)?;
    if !args_valid {
        return Err(invalid(name, "legacy args differ from the built-in Driver"));
    }
    if !evidence.1 || !evidence.2 {
        return Err(invalid(
            name,
            "legacy initialize/session evidence is incomplete",
        ));
    }
    if provider == ProviderId::Grok || provider == ProviderId::Cursor {
        verify_checksum(name, &path, &artifacts[0].digest)?;
    }
    let entry = exact_catalog_entry(catalog, provider, &identity, &artifacts).ok_or_else(|| {
        invalid(
            name,
            "installed artifact is not present in the active Catalog",
        )
    })?;
    let profile = match provider {
        ProviderId::Grok => ProviderProfile::Grok {
            executable: path,
            version_policy: VersionPolicy::Exact,
            catalog_entry: Some(entry.entry_id.clone()),
            permissions,
        },
        ProviderId::Cursor => ProviderProfile::Cursor {
            executable: path,
            version_policy: VersionPolicy::Exact,
            catalog_entry: Some(entry.entry_id.clone()),
            permissions,
        },
        ProviderId::Codex => ProviderProfile::Codex {
            adapter_path: path,
            version_policy: VersionPolicy::Exact,
            catalog_entry: Some(entry.entry_id.clone()),
            permissions,
        },
        ProviderId::Claude => ProviderProfile::Claude {
            adapter_path: path,
            version_policy: VersionPolicy::Exact,
            catalog_entry: Some(entry.entry_id.clone()),
            permissions,
        },
    };
    Ok((
        profile,
        vec![format!(
            "profile {name}: removed self-attested authentication/evidence ({})",
            evidence.0
        )],
    ))
}

fn exact_catalog_entry<'a>(
    catalog: &'a CompatibilityCatalog,
    provider: ProviderId,
    identity: &ProviderIdentity,
    artifacts: &[ArtifactDigest],
) -> Option<&'a CatalogEntry> {
    let mut expected = artifacts.to_vec();
    expected.sort_by(|left, right| left.subject.cmp(&right.subject));
    catalog.entries.iter().find(|entry| {
        let mut actual = entry.artifacts.clone();
        actual.sort_by(|left, right| left.subject.cmp(&right.subject));
        entry.provider == provider && &entry.identity == identity && actual == expected
    })
}

fn identity(version: String, component: Option<(&str, String)>) -> ProviderIdentity {
    ProviderIdentity {
        display_version: version.clone(),
        normalized_version: version,
        components: component
            .map(|(name, value)| [(name.to_owned(), value)].into_iter().collect())
            .unwrap_or_default(),
    }
}

fn artifact(subject: &str, digest: String) -> ArtifactDigest {
    ArtifactDigest {
        subject: subject.into(),
        algorithm: DigestAlgorithm::Sha256,
        digest: digest.to_ascii_lowercase(),
    }
}

fn adapter_artifacts(
    name: &str,
    path: &Path,
) -> Result<(Vec<ArtifactDigest>, PathBuf), ConfigError> {
    validate_absolute_file(name, path)?;
    let executable =
        std::fs::canonicalize(path).map_err(|error| invalid(name, error.to_string()))?;
    let package = executable
        .parent()
        .and_then(Path::parent)
        .map(|directory| directory.join("package.json"))
        .ok_or_else(|| invalid(name, "adapter package metadata path is unavailable"))?;
    validate_absolute_file(name, &package)?;
    Ok((
        vec![
            artifact("executable", sha256(&executable)?),
            artifact("package_metadata", sha256(&package)?),
        ],
        package,
    ))
}

fn read_secure_config(path: &Path) -> Result<String, ConfigError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ConfigError::InsecurePermissions(path.to_owned()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(ConfigError::InsecurePermissions(path.to_owned()));
        }
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge);
    }
    Ok(std::fs::read_to_string(path)?)
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

fn validate_absolute_file(name: &str, path: &Path) -> Result<(), ConfigError> {
    if !path.is_absolute() {
        return Err(invalid(name, "executable path must be absolute"));
    }
    let canonical =
        std::fs::canonicalize(path).map_err(|error| invalid(name, error.to_string()))?;
    let metadata =
        std::fs::symlink_metadata(canonical).map_err(|error| invalid(name, error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(name, "path must resolve to a regular file"));
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
    let actual = sha256(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(invalid(name, "sha256 does not match the pinned file"));
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String, ConfigError> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn atomic_replace_private(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(".providers.{}.tmp", uuid::Uuid::now_v7()));
    write_new_private(&temporary, bytes)?;
    std::fs::rename(temporary, path)?;
    std::fs::File::open(parent)?.sync_all()?;
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
