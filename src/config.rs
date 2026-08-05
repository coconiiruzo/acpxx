use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{PermissionPolicy, ProviderAssertions, ProviderId, ProviderSpec};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub const CONFIG_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub schema_version: u32,
    pub profiles: BTreeMap<String, ProviderProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderProfile {
    Grok {
        executable: PathBuf,
        #[serde(default, skip_serializing_if = "ProviderAssertions::is_empty")]
        assertions: ProviderAssertions,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Cursor {
        executable: PathBuf,
        #[serde(default, skip_serializing_if = "ProviderAssertions::is_empty")]
        assertions: ProviderAssertions,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Codex {
        adapter_path: PathBuf,
        #[serde(default, skip_serializing_if = "ProviderAssertions::is_empty")]
        assertions: ProviderAssertions,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Claude {
        adapter_path: PathBuf,
        #[serde(default, skip_serializing_if = "ProviderAssertions::is_empty")]
        assertions: ProviderAssertions,
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
    pub assertions: ProviderAssertions,
}

#[derive(Clone, Debug, Serialize)]
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
        let schema = schema_version(&value);
        match schema {
            None | Some(1) => return Err(ConfigError::MigrationRequired { found: schema }),
            Some(CONFIG_SCHEMA_VERSION) if is_catalog_era_shape(&value) => {
                return Err(ConfigError::MigrationRequired { found: schema });
            }
            Some(CONFIG_SCHEMA_VERSION) => {}
            Some(other) => return Err(ConfigError::UnsupportedSchema(other)),
        }
        let config: Self = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedProfile, ConfigError> {
        self.profiles
            .get(name)
            .ok_or_else(|| ConfigError::ProfileNotFound(name.to_owned()))?
            .resolve(name)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchema(self.schema_version));
        }
        for (name, profile) in &self.profiles {
            validate_profile_name(name)?;
            profile.validate(name)?;
        }
        Ok(())
    }
}

impl ProviderProfile {
    fn parts(&self) -> (ProviderId, &Path, &ProviderAssertions, ProfilePermissions) {
        match self {
            Self::Grok {
                executable,
                assertions,
                permissions,
            } => (ProviderId::Grok, executable, assertions, *permissions),
            Self::Cursor {
                executable,
                assertions,
                permissions,
            } => (ProviderId::Cursor, executable, assertions, *permissions),
            Self::Codex {
                adapter_path,
                assertions,
                permissions,
            } => (ProviderId::Codex, adapter_path, assertions, *permissions),
            Self::Claude {
                adapter_path,
                assertions,
                permissions,
            } => (ProviderId::Claude, adapter_path, assertions, *permissions),
        }
    }

    fn validate(&self, name: &str) -> Result<(), ConfigError> {
        let (_, path, assertions, _) = self.parts();
        validate_absolute_file(name, path)?;
        assertions
            .validate()
            .map_err(|message| invalid(name, message))
    }

    fn resolve(&self, name: &str) -> Result<ResolvedProfile, ConfigError> {
        self.validate(name)?;
        let (provider, path, assertions, permissions) = self.parts();
        let provider_spec = match provider {
            ProviderId::Grok => ProviderSpec::Grok {
                executable: Some(path.to_owned()),
            },
            ProviderId::Cursor => ProviderSpec::Cursor {
                executable: Some(path.to_owned()),
            },
            ProviderId::Codex => ProviderSpec::Codex {
                adapter: Some(path.to_owned()),
            },
            ProviderId::Claude => ProviderSpec::Claude {
                adapter: Some(path.to_owned()),
            },
        };
        Ok(ResolvedProfile {
            name: name.to_owned(),
            provider,
            provider_spec,
            permission_policy: match permissions {
                ProfilePermissions::Deny => PermissionPolicy::Deny,
                ProfilePermissions::AllowAll => PermissionPolicy::AllowAll,
            },
            assertions: assertions.clone(),
        })
    }
}

pub fn check_migration(path: impl AsRef<Path>) -> Result<ConfigMigration, ConfigError> {
    let content = read_secure_config(path.as_ref())?;
    let value: toml::Value = toml::from_str(&content)?;
    match schema_version(&value) {
        None | Some(1) => migrate_v1(toml::from_str(&content)?),
        Some(CONFIG_SCHEMA_VERSION) if is_catalog_era_shape(&value) => {
            migrate_catalog_v2(toml::from_str(&content)?)
        }
        Some(CONFIG_SCHEMA_VERSION) => {
            let config: ProviderConfig = toml::from_str(&content)?;
            config.validate()?;
            Ok(ConfigMigration {
                rendered: Some(toml::to_string_pretty(&config)?),
                blocking_issues: Vec::new(),
                warnings: vec!["configuration already uses final schema v2".into()],
            })
        }
        Some(other) => Err(ConfigError::UnsupportedSchema(other)),
    }
}

pub fn write_migration(path: impl AsRef<Path>) -> Result<PathBuf, ConfigError> {
    let path = path.as_ref();
    let migration = check_migration(path)?;
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
    let backup = path.with_extension(format!("toml.pre-runtime-compat-{timestamp}.bak"));
    write_new_private(&backup, &std::fs::read(path)?)?;
    atomic_replace_private(path, rendered.as_bytes())?;
    Ok(backup)
}

fn schema_version(value: &toml::Value) -> Option<u32> {
    value
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok())
}

fn is_catalog_era_shape(value: &toml::Value) -> bool {
    value.get("catalog").is_some()
        || value
            .get("profiles")
            .and_then(toml::Value::as_table)
            .is_some_and(|profiles| {
                profiles.values().any(|profile| {
                    profile.as_table().is_some_and(|profile| {
                        profile.contains_key("version_policy")
                            || profile.contains_key("catalog_entry")
                    })
                })
            })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV1Config {
    #[serde(default)]
    environment: Option<LegacyEnvironmentLock>,
    profiles: BTreeMap<String, LegacyV1Profile>,
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
enum LegacyV1Profile {
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

fn migrate_v1(legacy: LegacyV1Config) -> Result<ConfigMigration, ConfigError> {
    let mut warnings = Vec::new();
    if let Some(environment) = legacy.environment {
        warnings.push(format!(
            "removed legacy environment lock ({}, {}, {}, {})",
            environment.platform,
            environment.acp_protocol,
            environment.adapter_lockfile.display(),
            environment.adapter_lockfile_sha256
        ));
    }
    let mut profiles = BTreeMap::new();
    let mut issues = Vec::new();
    for (name, profile) in legacy.profiles {
        validate_profile_name(&name)?;
        match migrate_v1_profile(&name, profile) {
            Ok((profile, warning)) => {
                profiles.insert(name, profile);
                warnings.push(warning);
            }
            Err(error) => issues.push(error.to_string()),
        }
    }
    render_migration(profiles, issues, warnings)
}

fn migrate_v1_profile(
    name: &str,
    profile: LegacyV1Profile,
) -> Result<(ProviderProfile, String), ConfigError> {
    let (profile, auth, evidence) = match profile {
        LegacyV1Profile::Grok {
            executable,
            args,
            version,
            sha256,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => {
            if !args
                .iter()
                .map(String::as_str)
                .eq(["--no-auto-update", "agent", "stdio"])
            {
                return Err(invalid(name, "legacy args differ from the built-in Driver"));
            }
            (
                ProviderProfile::Grok {
                    executable,
                    assertions: ProviderAssertions {
                        version: Some(version),
                        launch_sha256: Some(sha256),
                        ..ProviderAssertions::default()
                    },
                    permissions,
                },
                authentication,
                (initialize_verified, session_new_verified),
            )
        }
        LegacyV1Profile::Cursor {
            executable,
            args,
            version,
            sha256,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => {
            if !args.iter().map(String::as_str).eq(["acp"]) {
                return Err(invalid(name, "legacy args differ from the built-in Driver"));
            }
            (
                ProviderProfile::Cursor {
                    executable,
                    assertions: ProviderAssertions {
                        version: Some(version),
                        launch_sha256: Some(sha256),
                        ..ProviderAssertions::default()
                    },
                    permissions,
                },
                authentication,
                (initialize_verified, session_new_verified),
            )
        }
        LegacyV1Profile::Codex {
            adapter_path,
            adapter_version,
            bundled_codex_version,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => (
            ProviderProfile::Codex {
                adapter_path,
                assertions: ProviderAssertions {
                    version: Some(adapter_version),
                    components: [("codex".into(), bundled_codex_version)]
                        .into_iter()
                        .collect(),
                    launch_sha256: None,
                },
                permissions,
            },
            authentication,
            (initialize_verified, session_new_verified),
        ),
        LegacyV1Profile::Claude {
            adapter_path,
            adapter_version,
            claude_agent_sdk_version,
            authentication,
            initialize_verified,
            session_new_verified,
            permissions,
        } => (
            ProviderProfile::Claude {
                adapter_path,
                assertions: ProviderAssertions {
                    version: Some(adapter_version),
                    components: [("claude_agent_sdk".into(), claude_agent_sdk_version)]
                        .into_iter()
                        .collect(),
                    launch_sha256: None,
                },
                permissions,
            },
            authentication,
            (initialize_verified, session_new_verified),
        ),
    };
    profile.validate(name)?;
    Ok((
        profile,
        format!(
            "profile {name}: old pin was retained as local assertions; remove the assertions table to try new provider versions automatically; removed self-attested authentication/evidence ({auth}, initialize={}, session/new={})",
            evidence.0, evidence.1
        ),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyCatalogV2Config {
    schema_version: u32,
    catalog: toml::Value,
    profiles: BTreeMap<String, LegacyCatalogProfile>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyPolicy {
    Verified,
    Exact,
    Experimental,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case", deny_unknown_fields)]
enum LegacyCatalogProfile {
    Grok {
        executable: PathBuf,
        #[serde(default = "default_verified")]
        version_policy: LegacyPolicy,
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Cursor {
        executable: PathBuf,
        #[serde(default = "default_verified")]
        version_policy: LegacyPolicy,
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Codex {
        adapter_path: PathBuf,
        #[serde(default = "default_verified")]
        version_policy: LegacyPolicy,
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
    Claude {
        adapter_path: PathBuf,
        #[serde(default = "default_verified")]
        version_policy: LegacyPolicy,
        catalog_entry: Option<String>,
        #[serde(default)]
        permissions: ProfilePermissions,
    },
}

const fn default_verified() -> LegacyPolicy {
    LegacyPolicy::Verified
}

fn migrate_catalog_v2(legacy: LegacyCatalogV2Config) -> Result<ConfigMigration, ConfigError> {
    if legacy.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchema(legacy.schema_version));
    }
    let _discarded_catalog = legacy.catalog;
    let mut profiles = BTreeMap::new();
    let mut issues = Vec::new();
    let mut warnings =
        vec!["removed Catalog source, signature, sequence, and update configuration".into()];
    for (name, profile) in legacy.profiles {
        let (provider, path, policy, entry, permissions) = match profile {
            LegacyCatalogProfile::Grok {
                executable,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Grok,
                executable,
                version_policy,
                catalog_entry,
                permissions,
            ),
            LegacyCatalogProfile::Cursor {
                executable,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Cursor,
                executable,
                version_policy,
                catalog_entry,
                permissions,
            ),
            LegacyCatalogProfile::Codex {
                adapter_path,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Codex,
                adapter_path,
                version_policy,
                catalog_entry,
                permissions,
            ),
            LegacyCatalogProfile::Claude {
                adapter_path,
                version_policy,
                catalog_entry,
                permissions,
            } => (
                ProviderId::Claude,
                adapter_path,
                version_policy,
                catalog_entry,
                permissions,
            ),
        };
        let assertions = match policy {
            LegacyPolicy::Verified | LegacyPolicy::Experimental => ProviderAssertions::default(),
            LegacyPolicy::Exact => match entry.as_deref().and_then(legacy_catalog_assertions) {
                Some(assertions) => assertions,
                None => {
                    issues.push(format!(
                        "profile {name}: exact Catalog entry could not be converted"
                    ));
                    continue;
                }
            },
        };
        let migrated = match provider {
            ProviderId::Grok => ProviderProfile::Grok {
                executable: path,
                assertions,
                permissions,
            },
            ProviderId::Cursor => ProviderProfile::Cursor {
                executable: path,
                assertions,
                permissions,
            },
            ProviderId::Codex => ProviderProfile::Codex {
                adapter_path: path,
                assertions,
                permissions,
            },
            ProviderId::Claude => ProviderProfile::Claude {
                adapter_path: path,
                assertions,
                permissions,
            },
        };
        if let Err(error) = migrated.validate(&name) {
            issues.push(error.to_string());
        } else {
            if matches!(policy, LegacyPolicy::Exact) {
                warnings.push(format!(
                    "profile {name}: exact Catalog entry was retained as local assertions"
                ));
            }
            profiles.insert(name, migrated);
        }
    }
    render_migration(profiles, issues, warnings)
}

fn legacy_catalog_assertions(entry: &str) -> Option<ProviderAssertions> {
    let (version, component, digest) = match entry {
        "grok/0.2.118/aarch64-apple-darwin/sha256-2de5b960" => (
            "0.2.118",
            None,
            "2de5b9609a03492dd6b9e4cca9637d651fe998bb8371bf9f852e7b28b38c034e",
        ),
        "cursor/2026.07.20-8cc9c0b/aarch64-apple-darwin/sha256-eed61c52" => (
            "2026.07.20-8cc9c0b",
            None,
            "eed61c5224668c9236334c4c68936a16aecc37374b592f59e31eb50433817831",
        ),
        "codex/1.1.9/aarch64-apple-darwin/sha256-c4fdf929" => (
            "1.1.9",
            Some(("codex", "0.145.0")),
            "c4fdf92936979fb1791d77437d75f934ed9c42e47738343620fcbe5c697cf1e6",
        ),
        "claude/0.64.2/aarch64-apple-darwin/sha256-260aac90" => (
            "0.64.2",
            Some(("claude_agent_sdk", "0.3.220")),
            "260aac90bf75f197b93640087c1de66441761d43c2784efa035fdcee60b5dacd",
        ),
        _ => return None,
    };
    Some(ProviderAssertions {
        version: Some(version.into()),
        components: component
            .map(|(name, value)| [(name.into(), value.into())].into_iter().collect())
            .unwrap_or_default(),
        launch_sha256: Some(digest.into()),
    })
}

fn render_migration(
    profiles: BTreeMap<String, ProviderProfile>,
    blocking_issues: Vec<String>,
    warnings: Vec<String>,
) -> Result<ConfigMigration, ConfigError> {
    let rendered = if blocking_issues.is_empty() {
        Some(toml::to_string_pretty(&ProviderConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            profiles,
        })?)
    } else {
        None
    };
    Ok(ConfigMigration {
        rendered,
        blocking_issues,
        warnings,
    })
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
