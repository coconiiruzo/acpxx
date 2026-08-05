mod claude;
mod codex;
mod cursor;
mod grok;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::compatibility::{ArtifactDigest, DriverId, ProviderIdentity};
use crate::{AdmissionError, PermissionPolicy, ProviderId, Task};

pub use claude::claude_driver;
pub use codex::codex_driver;
pub use cursor::cursor_driver;
pub use grok::grok_driver;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpVersionPolicy {
    StableV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySet(pub Vec<String>);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum IdentityProbe {
    Semver {
        args: Vec<String>,
    },
    ExactOutput {
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strip_prefix: Option<String>,
    },
}

impl IdentityProbe {
    #[must_use]
    pub fn args(&self) -> &[String] {
        match self {
            Self::Semver { args } | Self::ExactOutput { args, .. } => args,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProbe {
    LaunchExecutableSha256 {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package_metadata: Option<PackageMetadataProbe>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PackageMetadataProbe {
    pub package_name: String,
    pub component_dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderDriver {
    pub id: ProviderId,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub identity_probe: IdentityProbe,
    pub artifact_probe: ArtifactProbe,
    pub protocol: AcpVersionPolicy,
    pub required_capabilities: CapabilitySet,
    pub allowed_env: Vec<String>,
    pub fixed_env: BTreeMap<String, String>,
    pub preferred_auth_method: Option<String>,
    pub startup_timeout: Duration,
}

#[must_use]
pub fn all_provider_drivers() -> [ProviderDriver; 4] {
    [
        codex_driver(None),
        claude_driver(None),
        grok_driver(None),
        cursor_driver(None),
    ]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedProvider {
    pub identity: ProviderIdentity,
    pub artifacts: Vec<ArtifactDigest>,
    pub executable: PathBuf,
    pub executable_identity: ExecutableFileIdentity,
    pub qualified_files: Vec<QualifiedArtifactFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualifiedArtifactFile {
    pub subject: String,
    pub path: PathBuf,
    pub identity: ExecutableFileIdentity,
    pub executable_required: bool,
}

impl ObservedProvider {
    pub fn verify_qualified_files(&self) -> std::io::Result<()> {
        for file in &self.qualified_files {
            file.identity
                .verify_artifact_path(&file.path, file.executable_required)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableFileIdentity {
    pub owner: u32,
    pub mode: u32,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
}

impl ExecutableFileIdentity {
    pub fn from_path(path: &std::path::Path) -> std::io::Result<Self> {
        Self::from_artifact_path(path, true)
    }

    pub fn from_artifact_path(
        path: &std::path::Path,
        executable_required: bool,
    ) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider executable is not a regular file",
            ));
        }
        let owner = metadata.uid();
        if owner != unsafe { libc::geteuid() } && owner != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "provider artifact must be owned by the current user or root",
            ));
        }
        let mode = metadata.mode();
        if mode & 0o022 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "provider artifact must not be group/world writable",
            ));
        }
        if executable_required && mode & 0o111 == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "provider launch artifact is not executable",
            ));
        }
        Ok(Self {
            owner,
            mode,
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }

    pub fn verify_path(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.verify_artifact_path(path, true)
    }

    pub fn verify_artifact_path(
        &self,
        path: &std::path::Path,
        executable_required: bool,
    ) -> std::io::Result<()> {
        let current = Self::from_artifact_path(path, executable_required)?;
        if &current == self {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider executable changed after artifact qualification",
            ))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ProviderSpec {
    Grok {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executable: Option<PathBuf>,
    },
    Cursor {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executable: Option<PathBuf>,
    },
    Codex {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        adapter: Option<PathBuf>,
    },
    Claude {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        adapter: Option<PathBuf>,
    },
}

impl ProviderSpec {
    #[must_use]
    pub fn grok() -> Self {
        Self::Grok { executable: None }
    }

    pub fn driver(&self) -> std::result::Result<ProviderDriver, AdmissionError> {
        match self {
            Self::Grok { executable } => Ok(grok_driver(executable.clone())),
            Self::Cursor { executable } => Ok(cursor_driver(executable.clone())),
            Self::Codex { adapter } => Ok(codex_driver(adapter.clone())),
            Self::Claude { adapter } => Ok(claude_driver(adapter.clone())),
        }
    }

    #[must_use]
    pub fn id(&self) -> ProviderId {
        match self {
            Self::Grok { .. } => ProviderId::Grok,
            Self::Cursor { .. } => ProviderId::Cursor,
            Self::Codex { .. } => ProviderId::Codex,
            Self::Claude { .. } => ProviderId::Claude,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub provider: ProviderSpec,
    pub cwd: PathBuf,
    pub task: Task,
    pub permission_policy: PermissionPolicy,
    #[serde(default)]
    pub version_policy: crate::VersionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_entry: Option<String>,
    #[serde(default)]
    pub allow_unverified_mutations: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListQuery {
    pub agent_id: Option<crate::AgentId>,
    pub run_id: Option<crate::RunId>,
    pub provider: Option<ProviderId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderSnapshot {
    pub id: ProviderId,
    pub protocol: AcpVersionPolicy,
    pub driver_id: DriverId,
    pub driver_revision: u32,
    pub required_capabilities: CapabilitySet,
    pub recommended: Option<crate::RecommendedCatalogEntry>,
    pub catalog_sequence: u64,
    pub catalog_digest: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListSnapshot {
    pub agents: Vec<crate::AgentSnapshot>,
    pub runs: Vec<crate::RunSnapshot>,
    pub providers: Vec<ProviderSnapshot>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WaitOptions {
    pub timeout: Option<Duration>,
}
