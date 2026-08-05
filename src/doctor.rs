use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ProviderDriver, ProviderId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorReport {
    pub agentmux_version: String,
    pub acp_sdk_version: String,
    pub acp_protocol: String,
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Warning,
    Fail,
}

pub async fn inspect(socket: &Path, database: &Path) -> DoctorReport {
    inspect_with_config(socket, database, &crate::config::default_config_path()).await
}

pub async fn inspect_with_config(socket: &Path, database: &Path, config: &Path) -> DoctorReport {
    let mut checks = Vec::new();
    let catalog_store = catalog_store(config);
    match &catalog_store {
        Ok(store) => checks.push(catalog_check(store)),
        Err(error) => checks.push(DoctorCheck {
            name: "compatibility.catalog".into(),
            status: DoctorStatus::Fail,
            message: error.clone(),
        }),
    }
    let catalog = catalog_store
        .as_ref()
        .ok()
        .map(|store| store.snapshot().catalog);
    let configured = crate::config::ProviderConfig::load(config).ok();
    if let Some(configured) = &configured {
        for profile in configured.profiles.keys() {
            match configured.resolve(profile) {
                Ok(resolved) => match resolved.provider_spec.driver() {
                    Ok(driver) => {
                        let mut check = version_check(&driver, catalog.clone()).await;
                        check.name = format!("profile.{profile}.compatibility");
                        checks.push(check);
                    }
                    Err(error) => checks.push(DoctorCheck {
                        name: format!("profile.{profile}.compatibility"),
                        status: DoctorStatus::Fail,
                        message: error.to_string(),
                    }),
                },
                Err(error) => checks.push(DoctorCheck {
                    name: format!("profile.{profile}.compatibility"),
                    status: DoctorStatus::Fail,
                    message: error.to_string(),
                }),
            }
        }
    } else {
        for driver in crate::all_provider_drivers() {
            checks.push(version_check(&driver, catalog.clone()).await);
        }
    }
    for provider in [
        ProviderId::Codex,
        ProviderId::Claude,
        ProviderId::Grok,
        ProviderId::Cursor,
    ] {
        checks.push(authentication_check(provider).await);
    }
    checks.push(socket_check(socket));
    checks.push(database_check(database));
    checks.push(config_check(config));
    let healthy = checks
        .iter()
        .all(|check| check.status != DoctorStatus::Fail);
    DoctorReport {
        agentmux_version: env!("CARGO_PKG_VERSION").into(),
        acp_sdk_version: "2.0.0".into(),
        acp_protocol: "v1".into(),
        healthy,
        checks,
    }
}

fn config_check(path: &Path) -> DoctorCheck {
    if !path.exists() {
        return DoctorCheck {
            name: "config.provider_profiles".into(),
            status: DoctorStatus::Warning,
            message: format!("{} does not exist", path.display()),
        };
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.permissions().mode() & 0o777 != 0o600 => {
                return DoctorCheck {
                    name: "config.provider_profiles".into(),
                    status: DoctorStatus::Fail,
                    message: format!("{} must be mode 0600", path.display()),
                };
            }
            Err(error) => {
                return DoctorCheck {
                    name: "config.provider_profiles".into(),
                    status: DoctorStatus::Fail,
                    message: error.to_string(),
                };
            }
            _ => {}
        }
    }
    match crate::config::ProviderConfig::load(path) {
        Ok(config) => {
            for name in config.profiles.keys() {
                if let Err(error) = config.resolve(name) {
                    return DoctorCheck {
                        name: "config.provider_profiles".into(),
                        status: DoctorStatus::Fail,
                        message: error.to_string(),
                    };
                }
            }
            DoctorCheck {
                name: "config.provider_profiles".into(),
                status: DoctorStatus::Pass,
                message: format!(
                    "{} contains {} schema-v2 Catalog-governed profiles",
                    path.display(),
                    config.profiles.len()
                ),
            }
        }
        Err(error) => DoctorCheck {
            name: "config.provider_profiles".into(),
            status: DoctorStatus::Fail,
            message: error.to_string(),
        },
    }
}

async fn version_check(
    driver: &ProviderDriver,
    catalog: Option<std::sync::Arc<crate::VerifiedCatalog>>,
) -> DoctorCheck {
    match crate::acp::observe_provider(driver).await {
        Ok(observed) => {
            let Some(catalog) = catalog else {
                return DoctorCheck {
                    name: format!("{}.compatibility", driver.id),
                    status: DoctorStatus::Fail,
                    message: "Catalog is unavailable; identity was observed but not authorized"
                        .into(),
                };
            };
            let target = crate::host_target();
            let agentmux_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("package version must be semver");
            match crate::resolve_provider(
                &catalog,
                &crate::ResolutionRequest {
                    driver,
                    observed: &observed,
                    target,
                    policy: crate::VersionPolicy::Verified,
                    exact_entry: None,
                    agentmux_version: &agentmux_version,
                    now: time::OffsetDateTime::now_utc(),
                },
            ) {
                Ok(provider_lock) => DoctorCheck {
                    name: format!("{}.compatibility", driver.id),
                    status: match provider_lock.compatibility {
                        crate::CompatibilityLevel::Verified => DoctorStatus::Pass,
                        crate::CompatibilityLevel::Deprecated
                        | crate::CompatibilityLevel::Experimental => DoctorStatus::Warning,
                    },
                    message: format!(
                        "{} reports {} selected as {:?} by Catalog entry {} (recommended: {}; Driver {} revision {})",
                        observed.executable.display(),
                        observed.identity.display_version,
                        provider_lock.compatibility,
                        provider_lock.catalog_entry_id.as_deref().unwrap_or("none"),
                        recommended_identity(&catalog, driver).unwrap_or("unavailable"),
                        driver.driver_id.0,
                        driver.driver_revision
                    ),
                },
                Err(error) => DoctorCheck {
                    name: format!("{}.compatibility", driver.id),
                    status: DoctorStatus::Fail,
                    message: format!(
                        "installed identity {} is not usable: {error}",
                        observed.identity.display_version
                    ),
                },
            }
        }
        Err(failure) => DoctorCheck {
            name: format!("{}.compatibility", driver.id),
            status: DoctorStatus::Fail,
            message: failure.message,
        },
    }
}

fn recommended_identity<'a>(
    catalog: &'a crate::VerifiedCatalog,
    driver: &ProviderDriver,
) -> Option<&'a str> {
    let target = crate::host_target();
    let channel = catalog.catalog.channels.iter().find(|channel| {
        channel.provider == driver.id && channel.target == target && channel.name == "recommended"
    })?;
    catalog
        .catalog
        .entries
        .iter()
        .find(|entry| entry.entry_id == channel.entry_id)
        .map(|entry| entry.identity.display_version.as_str())
}

fn catalog_store(config: &Path) -> Result<crate::CatalogStore, String> {
    let bootstrap = crate::bootstrap_catalog().map_err(|error| error.to_string())?;
    let keyring = crate::official_keyring().map_err(|error| error.to_string())?;
    if config.exists() {
        let config = crate::config::ProviderConfig::load(config).map_err(|error| match error {
            crate::config::ConfigError::MigrationRequired { .. } => {
                format!("{error}; run `agentmux config migrate --check` before doctor/serve")
            }
            _ => error.to_string(),
        })?;
        if config.catalog.source == crate::config::CatalogSourceConfig::File {
            return crate::CatalogStore::open_file(
                config.catalog.path.expect("validated Catalog file path"),
                config
                    .catalog
                    .signature_path
                    .expect("validated Catalog signature path"),
                bootstrap,
                keyring,
            )
            .map_err(|error| error.to_string());
        }
    }
    crate::CatalogStore::open(crate::default_catalog_cache_path(), bootstrap, keyring)
        .map_err(|error| error.to_string())
}

fn catalog_check(store: &crate::CatalogStore) -> DoctorCheck {
    let status = store.status();
    let expires = time::OffsetDateTime::parse(
        &status.expires_at,
        &time::format_description::well_known::Rfc3339,
    );
    let expired = expires.is_ok_and(|expires| expires <= time::OffsetDateTime::now_utc());
    DoctorCheck {
        name: "compatibility.catalog".into(),
        status: if expired {
            DoctorStatus::Fail
        } else if !status.unsupported_driver_entries.is_empty() {
            DoctorStatus::Warning
        } else {
            DoctorStatus::Pass
        },
        message: format!(
            "{:?} Catalog {} sequence {} digest {} signed by {:?}, expires {}; unsupported Driver entries: {:?}",
            status.source,
            status.catalog_id,
            status.sequence,
            status.digest,
            status.signature_key_ids,
            status.expires_at,
            status.unsupported_driver_entries
        ),
    }
}

async fn authentication_check(provider: ProviderId) -> DoctorCheck {
    let (command, args): (&str, &[&str]) = match provider {
        ProviderId::Grok => ("grok", &["models"]),
        ProviderId::Cursor => ("cursor-agent", &["status"]),
        ProviderId::Codex => ("codex", &["login", "status"]),
        ProviderId::Claude => ("claude", &["auth", "status", "--json"]),
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    )
    .await;
    match result {
        Ok(Ok(status)) if status.success() => DoctorCheck {
            name: format!("{provider}.authentication"),
            status: DoctorStatus::Pass,
            message: "authentication status command succeeded".into(),
        },
        Ok(Ok(status)) => DoctorCheck {
            name: format!("{provider}.authentication"),
            status: DoctorStatus::Fail,
            message: format!("authentication status exited with {status}"),
        },
        Ok(Err(error)) => DoctorCheck {
            name: format!("{provider}.authentication"),
            status: DoctorStatus::Fail,
            message: error.to_string(),
        },
        Err(_) => DoctorCheck {
            name: format!("{provider}.authentication"),
            status: DoctorStatus::Fail,
            message: "authentication status timed out".into(),
        },
    }
}

fn socket_check(socket: &Path) -> DoctorCheck {
    if socket.exists() {
        let secure = crate::ipc::socket_is_user_only(socket);
        DoctorCheck {
            name: "ipc.socket_permissions".into(),
            status: if secure {
                DoctorStatus::Pass
            } else {
                DoctorStatus::Fail
            },
            message: if secure {
                format!("{} is mode 0600", socket.display())
            } else {
                format!("{} is not mode 0600", socket.display())
            },
        }
    } else {
        DoctorCheck {
            name: "ipc.socket_permissions".into(),
            status: DoctorStatus::Warning,
            message: format!("{} does not exist; broker may be stopped", socket.display()),
        }
    }
}

fn database_check(database: &Path) -> DoctorCheck {
    if !database.exists() {
        return DoctorCheck {
            name: "sqlite.metadata_store".into(),
            status: DoctorStatus::Warning,
            message: format!("{} does not exist yet", database.display()),
        };
    }
    match rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .and_then(|connection| {
        connection.query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |row| row.get::<_, i64>(0),
        )
    }) {
        Ok(crate::storage::SCHEMA_VERSION) => DoctorCheck {
            name: "sqlite.metadata_store".into(),
            status: DoctorStatus::Pass,
            message: format!(
                "{} uses schema version {}",
                database.display(),
                crate::storage::SCHEMA_VERSION
            ),
        },
        Ok(version) => DoctorCheck {
            name: "sqlite.metadata_store".into(),
            status: DoctorStatus::Fail,
            message: format!("unsupported schema version {version}"),
        },
        Err(error) => DoctorCheck {
            name: "sqlite.metadata_store".into(),
            status: DoctorStatus::Fail,
            message: error.to_string(),
        },
    }
}

#[must_use]
pub fn default_database_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Application Support/agentmux/metadata.sqlite3")
}
