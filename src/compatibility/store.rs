use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CatalogEntryState, CatalogError, CatalogKeyring, VerifiedCatalog};

const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    Bootstrap,
    Cache,
    File,
}

#[derive(Clone, Debug)]
pub struct CatalogSnapshot {
    pub source: CatalogSource,
    pub catalog: Arc<VerifiedCatalog>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecommendedCatalogEntry {
    pub provider: crate::ProviderId,
    pub target: String,
    pub channel: String,
    pub entry_id: String,
    pub display_version: String,
    pub driver_id: super::DriverId,
    pub driver_revision: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogStatus {
    pub source: CatalogSource,
    pub catalog_id: String,
    pub sequence: u64,
    pub digest: String,
    pub generated_at: String,
    pub expires_at: String,
    pub signature_key_ids: Vec<String>,
    pub highest_accepted_sequence: u64,
    pub lkg_available: bool,
    pub recommended: Vec<RecommendedCatalogEntry>,
    pub unsupported_driver_entries: Vec<String>,
}

#[derive(Clone)]
pub struct CatalogStore {
    root: PathBuf,
    bootstrap: Arc<VerifiedCatalog>,
    keyring: CatalogKeyring,
    active: Arc<RwLock<CatalogSnapshot>>,
    file_source: Option<(PathBuf, PathBuf)>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogState {
    schema_version: u32,
    catalog_id: String,
    highest_sequence: u64,
    catalog_digest: String,
    active_generation: String,
}

impl CatalogStore {
    pub fn open(
        root: impl Into<PathBuf>,
        bootstrap: VerifiedCatalog,
        keyring: CatalogKeyring,
    ) -> Result<Self, CatalogError> {
        let root = root.into();
        validate_root(&root)?;
        let bootstrap = Arc::new(bootstrap);
        let active = load_active(&root, &bootstrap, &keyring)?;
        Ok(Self {
            root,
            bootstrap,
            keyring,
            active: Arc::new(RwLock::new(active)),
            file_source: None,
        })
    }

    pub fn open_file(
        catalog_path: impl Into<PathBuf>,
        signature_path: impl Into<PathBuf>,
        bootstrap: VerifiedCatalog,
        keyring: CatalogKeyring,
    ) -> Result<Self, CatalogError> {
        let catalog_path = catalog_path.into();
        let signature_path = signature_path.into();
        let catalog = VerifiedCatalog::verify(
            &std::fs::read(&catalog_path).map_err(cache)?,
            &std::fs::read(&signature_path).map_err(cache)?,
            &keyring,
        )?;
        validate_runtime_catalog(&catalog)?;
        if catalog.catalog.catalog_id != bootstrap.catalog.catalog_id {
            return Err(CatalogError::Cache(
                "file Catalog ID does not match bootstrap trust root".into(),
            ));
        }
        Ok(Self {
            root: catalog_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
            bootstrap: Arc::new(bootstrap),
            keyring,
            active: Arc::new(RwLock::new(CatalogSnapshot {
                source: CatalogSource::File,
                catalog: Arc::new(catalog),
            })),
            file_source: Some((catalog_path, signature_path)),
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> CatalogSnapshot {
        self.active
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[must_use]
    pub fn status(&self) -> CatalogStatus {
        status_from_snapshot(&self.snapshot(), &self.root)
    }

    pub fn install_verified(
        &self,
        catalog_bytes: &[u8],
        signature_bytes: &[u8],
    ) -> Result<CatalogSnapshot, CatalogError> {
        if self.file_source.is_some() {
            return Err(CatalogError::Cache(
                "cannot install into a read-only file Catalog source".into(),
            ));
        }
        let candidate = VerifiedCatalog::verify(catalog_bytes, signature_bytes, &self.keyring)?;
        validate_runtime_catalog(&candidate)?;
        if candidate.catalog.catalog_id != self.bootstrap.catalog.catalog_id {
            return Err(CatalogError::Cache(format!(
                "Catalog ID {:?} does not match trust root {:?}",
                candidate.catalog.catalog_id, self.bootstrap.catalog.catalog_id
            )));
        }
        let current = self.snapshot();
        if candidate.catalog.sequence <= current.catalog.catalog.sequence {
            return Err(CatalogError::Rollback {
                candidate: candidate.catalog.sequence,
                accepted: current.catalog.catalog.sequence,
            });
        }

        ensure_cache_root(&self.root)?;
        let generation = generation_name(candidate.catalog.sequence, &candidate.digest);
        let generations = self.root.join("generations");
        let destination = generations.join(&generation);
        if destination.exists() {
            return Err(CatalogError::Cache(format!(
                "immutable Catalog generation already exists: {}",
                destination.display()
            )));
        }
        let temporary = generations.join(format!(".install-{}", Uuid::now_v7()));
        std::fs::create_dir(&temporary).map_err(cache)?;
        set_private_directory(&temporary)?;
        let install: Result<(), CatalogError> = (|| {
            write_new(&temporary.join("catalog.json"), catalog_bytes)?;
            write_new(&temporary.join("catalog.sig"), signature_bytes)?;
            sync_directory(&temporary)?;
            std::fs::rename(&temporary, &destination).map_err(cache)?;
            sync_directory(&generations)?;

            let state = CatalogState {
                schema_version: STATE_SCHEMA_VERSION,
                catalog_id: candidate.catalog.catalog_id.clone(),
                highest_sequence: candidate.catalog.sequence,
                catalog_digest: candidate.digest.clone(),
                active_generation: generation,
            };
            let state_bytes = serde_json::to_vec_pretty(&state).map_err(|error| {
                CatalogError::Cache(format!("failed to serialize Catalog state: {error}"))
            })?;
            atomic_write(&self.root, "state.json", &state_bytes)?;
            atomic_write(&self.root, "state.lkg.json", &state_bytes)?;
            sync_directory(&self.root)?;
            Ok(())
        })();
        if install.is_err() {
            let _ = std::fs::remove_dir_all(&temporary);
        }
        install?;

        let snapshot = CatalogSnapshot {
            source: CatalogSource::Cache,
            catalog: Arc::new(candidate),
        };
        *self
            .active
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.clone();
        Ok(snapshot)
    }

    pub fn reload(&self) -> Result<CatalogSnapshot, CatalogError> {
        if let Some((catalog_path, signature_path)) = &self.file_source {
            let candidate = VerifiedCatalog::verify(
                &std::fs::read(catalog_path).map_err(cache)?,
                &std::fs::read(signature_path).map_err(cache)?,
                &self.keyring,
            )?;
            validate_runtime_catalog(&candidate)?;
            if candidate.catalog.catalog_id != self.bootstrap.catalog.catalog_id {
                return Err(CatalogError::Cache(
                    "file Catalog ID does not match bootstrap trust root".into(),
                ));
            }
            let snapshot = CatalogSnapshot {
                source: CatalogSource::File,
                catalog: Arc::new(candidate),
            };
            *self
                .active
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.clone();
            return Ok(snapshot);
        }
        let loaded = load_active(&self.root, &self.bootstrap, &self.keyring)?;
        let current = self.snapshot();
        if loaded.catalog.catalog.sequence < current.catalog.catalog.sequence {
            return Err(CatalogError::Rollback {
                candidate: loaded.catalog.catalog.sequence,
                accepted: current.catalog.catalog.sequence,
            });
        }
        *self
            .active
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = loaded.clone();
        Ok(loaded)
    }
}

#[must_use]
pub fn status_from_snapshot(snapshot: &CatalogSnapshot, root: &Path) -> CatalogStatus {
    let catalog = &snapshot.catalog.catalog;
    let mut recommended = Vec::new();
    for channel in &catalog.channels {
        if let Some(entry) = catalog.entries.iter().find(|entry| {
            entry.entry_id == channel.entry_id && entry.state != CatalogEntryState::Blocked
        }) {
            recommended.push(RecommendedCatalogEntry {
                provider: entry.provider,
                target: entry.target.clone(),
                channel: channel.name.clone(),
                entry_id: entry.entry_id.clone(),
                display_version: entry.identity.display_version.clone(),
                driver_id: entry.driver_id.clone(),
                driver_revision: entry.driver_revision,
            });
        }
    }
    recommended.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.channel.cmp(&right.channel))
    });
    let drivers = crate::all_provider_drivers();
    let unsupported_driver_entries = catalog
        .entries
        .iter()
        .filter(|entry| {
            !drivers.iter().any(|driver| {
                driver.driver_id == entry.driver_id
                    && driver.driver_revision == entry.driver_revision
            })
        })
        .map(|entry| entry.entry_id.clone())
        .collect();
    CatalogStatus {
        source: snapshot.source,
        catalog_id: catalog.catalog_id.clone(),
        sequence: catalog.sequence,
        digest: snapshot.catalog.digest.clone(),
        generated_at: catalog.generated_at.clone(),
        expires_at: catalog.expires_at.clone(),
        signature_key_ids: snapshot.catalog.signer_key_ids.clone(),
        highest_accepted_sequence: catalog.sequence,
        lkg_available: snapshot.source == CatalogSource::Cache
            && root.join("state.lkg.json").is_file(),
        recommended,
        unsupported_driver_entries,
    }
}

#[must_use]
pub fn default_catalog_cache_path() -> PathBuf {
    if let Some(root) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(root).join("agentmux/compatibility");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Application Support/agentmux/compatibility")
}

fn load_active(
    root: &Path,
    bootstrap: &Arc<VerifiedCatalog>,
    keyring: &CatalogKeyring,
) -> Result<CatalogSnapshot, CatalogError> {
    let primary = root.join("state.json");
    let lkg = root.join("state.lkg.json");
    if primary.exists() || lkg.exists() {
        let primary_result = load_state(&primary).and_then(|state| {
            load_generation(root, &state, bootstrap, keyring).map(|catalog| (state, catalog))
        });
        match primary_result {
            Ok((_, catalog)) => {
                return Ok(CatalogSnapshot {
                    source: CatalogSource::Cache,
                    catalog: Arc::new(catalog),
                });
            }
            Err(primary_error) => {
                let (state, catalog) = load_state(&lkg)
                    .and_then(|state| {
                        load_generation(root, &state, bootstrap, keyring)
                            .map(|catalog| (state, catalog))
                    })
                    .map_err(|lkg_error| {
                        CatalogError::Cache(format!(
                            "active state failed ({primary_error}); redundant state failed ({lkg_error})"
                        ))
                    })?;
                let _ = state;
                return Ok(CatalogSnapshot {
                    source: CatalogSource::Cache,
                    catalog: Arc::new(catalog),
                });
            }
        }
    }

    if let Some(state) = highest_generation_state(root, &bootstrap.catalog.catalog_id)? {
        let catalog = load_generation(root, &state, bootstrap, keyring)?;
        return Ok(CatalogSnapshot {
            source: CatalogSource::Cache,
            catalog: Arc::new(catalog),
        });
    }

    Ok(CatalogSnapshot {
        source: CatalogSource::Bootstrap,
        catalog: bootstrap.clone(),
    })
}

fn load_state(path: &Path) -> Result<CatalogState, CatalogError> {
    let bytes = read_cache_file(path)?;
    let state: CatalogState = serde_json::from_slice(&bytes)
        .map_err(|error| CatalogError::Cache(format!("invalid {}: {error}", path.display())))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(CatalogError::Cache(format!(
            "unsupported Catalog state schema {}",
            state.schema_version
        )));
    }
    if state.active_generation != generation_name(state.highest_sequence, &state.catalog_digest) {
        return Err(CatalogError::Cache(
            "Catalog state generation does not match sequence/digest".into(),
        ));
    }
    Ok(state)
}

fn load_generation(
    root: &Path,
    state: &CatalogState,
    bootstrap: &VerifiedCatalog,
    keyring: &CatalogKeyring,
) -> Result<VerifiedCatalog, CatalogError> {
    if state.catalog_id != bootstrap.catalog.catalog_id {
        return Err(CatalogError::Cache(
            "cached Catalog ID does not match bootstrap trust root".into(),
        ));
    }
    let directory = root.join("generations").join(&state.active_generation);
    validate_root(&directory)?;
    let catalog_bytes = read_cache_file(&directory.join("catalog.json"))?;
    let signature_bytes = read_cache_file(&directory.join("catalog.sig"))?;
    let verified = VerifiedCatalog::verify(&catalog_bytes, &signature_bytes, keyring)?;
    validate_runtime_catalog(&verified)?;
    if verified.catalog.catalog_id != state.catalog_id
        || verified.catalog.sequence != state.highest_sequence
        || verified.digest != state.catalog_digest
    {
        return Err(CatalogError::Cache(
            "cached Catalog does not match active state".into(),
        ));
    }
    Ok(verified)
}

fn highest_generation_state(
    root: &Path,
    catalog_id: &str,
) -> Result<Option<CatalogState>, CatalogError> {
    let generations = root.join("generations");
    let entries = match std::fs::read_dir(&generations) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(cache(error)),
    };
    let mut highest: Option<(u64, String, String)> = None;
    for entry in entries {
        let entry = entry.map_err(cache)?;
        if !entry.file_type().map_err(cache)?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Some((sequence, digest)) = parse_generation_name(&name) else {
            return Err(CatalogError::Cache(format!(
                "invalid immutable generation name {name:?}"
            )));
        };
        if highest
            .as_ref()
            .is_none_or(|(accepted, _, _)| sequence > *accepted)
        {
            highest = Some((sequence, digest, name));
        }
    }
    Ok(highest.map(|(sequence, digest, name)| CatalogState {
        schema_version: STATE_SCHEMA_VERSION,
        catalog_id: catalog_id.to_owned(),
        highest_sequence: sequence,
        catalog_digest: digest,
        active_generation: name,
    }))
}

fn parse_generation_name(value: &str) -> Option<(u64, String)> {
    let (sequence, digest) = value.split_once('-')?;
    let sequence = sequence.parse().ok()?;
    super::validate_sha256(digest).ok()?;
    Some((sequence, digest.to_owned()))
}

fn generation_name(sequence: u64, digest: &str) -> String {
    format!("{sequence}-{digest}")
}

fn validate_root(root: &Path) -> Result<(), CatalogError> {
    if let Ok(metadata) = std::fs::symlink_metadata(root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CatalogError::Cache(format!(
                "Catalog cache root must be a real directory: {}",
                root.display()
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(CatalogError::Cache(format!(
                    "Catalog cache root must be current-user owned and mode 0700: {}",
                    root.display()
                )));
            }
        }
    }
    Ok(())
}

fn ensure_cache_root(root: &Path) -> Result<(), CatalogError> {
    std::fs::create_dir_all(root).map_err(cache)?;
    set_private_directory(root)?;
    validate_root(root)?;
    let generations = root.join("generations");
    std::fs::create_dir_all(&generations).map_err(cache)?;
    set_private_directory(&generations)?;
    validate_root(&generations)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), CatalogError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(cache)?;
    file.write_all(bytes).map_err(cache)?;
    file.sync_all().map_err(cache)
}

fn atomic_write(root: &Path, name: &str, bytes: &[u8]) -> Result<(), CatalogError> {
    let temporary = root.join(format!(".{name}.{}", Uuid::now_v7()));
    let destination = root.join(name);
    let result = (|| {
        write_new(&temporary, bytes)?;
        std::fs::rename(&temporary, &destination).map_err(cache)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), CatalogError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(cache)
}

fn set_private_directory(path: &Path) -> Result<(), CatalogError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(cache)?;
    }
    Ok(())
}

fn cache(error: std::io::Error) -> CatalogError {
    CatalogError::Cache(error.to_string())
}

fn read_cache_file(path: &Path) -> Result<Vec<u8>, CatalogError> {
    let metadata = std::fs::symlink_metadata(path).map_err(cache)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CatalogError::Cache(format!(
            "Catalog cache file must be a regular file: {}",
            path.display()
        )));
    }
    std::fs::read(path).map_err(cache)
}

fn validate_runtime_catalog(catalog: &VerifiedCatalog) -> Result<(), CatalogError> {
    let drivers = crate::all_provider_drivers()
        .into_iter()
        .map(|driver| (driver.driver_id, driver.driver_revision))
        .collect();
    let target = super::host_target();
    let agentmux_version =
        semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("package version must be semver");
    catalog
        .catalog
        .validate_for(&super::CatalogValidationContext {
            target,
            agentmux_version: &agentmux_version,
            drivers: &drivers,
            now: time::OffsetDateTime::now_utc(),
        })
}
