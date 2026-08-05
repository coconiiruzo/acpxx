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
    if config.exists() {
        match crate::config::ProviderConfig::load(config) {
            Ok(provider_config) => {
                for name in provider_config.profiles.keys() {
                    match provider_config.resolve(name) {
                        Ok(profile) => match profile.provider_spec.driver() {
                            Ok(driver) => {
                                checks
                                    .push(provider_check(name, &driver, &profile.assertions).await);
                            }
                            Err(error) => checks.push(DoctorCheck {
                                name: format!("profile.{name}"),
                                status: DoctorStatus::Fail,
                                message: error.to_string(),
                            }),
                        },
                        Err(error) => checks.push(DoctorCheck {
                            name: format!("profile.{name}"),
                            status: DoctorStatus::Fail,
                            message: error.to_string(),
                        }),
                    }
                }
            }
            Err(error) => checks.push(DoctorCheck {
                name: "config.provider_profiles".into(),
                status: DoctorStatus::Fail,
                message: error.to_string(),
            }),
        }
    } else {
        checks.push(config_check(config));
    }
    for provider in [
        ProviderId::Grok,
        ProviderId::Cursor,
        ProviderId::Codex,
        ProviderId::Claude,
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
                    "{} contains {} provider profiles",
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

async fn provider_check(
    name: &str,
    driver: &ProviderDriver,
    assertions: &crate::ProviderAssertions,
) -> DoctorCheck {
    match crate::acp::observe_provider(driver).await {
        Ok(observed) => {
            let assertion_result = observed.assertion_result(assertions);
            let status = if matches!(assertion_result, crate::AssertionResult::Failed { .. }) {
                DoctorStatus::Fail
            } else if observed.version.observed().is_some() {
                DoctorStatus::Pass
            } else {
                DoctorStatus::Warning
            };
            DoctorCheck {
                name: format!("profile.{name}"),
                status,
                message: format!(
                    "{} is safe; observed version: {:?}; assertions: {:?}",
                    observed.executable.display(),
                    observed.version,
                    assertion_result
                ),
            }
        }
        Err(failure) => DoctorCheck {
            name: format!("profile.{name}"),
            status: DoctorStatus::Fail,
            message: failure.message,
        },
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
        Ok(3) => DoctorCheck {
            name: "sqlite.metadata_store".into(),
            status: DoctorStatus::Pass,
            message: format!("{} uses schema version 3", database.display()),
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
