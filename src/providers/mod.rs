mod grok;

use std::path::PathBuf;
use std::time::Duration;

use semver::VersionReq;
use serde::{Deserialize, Serialize};

use crate::{PermissionPolicy, ProviderId, Task};

pub use grok::{GROK_TESTED_VERSION, grok_manifest};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpVersionPolicy {
    StableV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySet(pub Vec<String>);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderManifest {
    pub id: ProviderId,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub version_args: Vec<String>,
    pub expected_version: VersionReq,
    pub protocol: AcpVersionPolicy,
    pub required_capabilities: CapabilitySet,
    pub allowed_env: Vec<String>,
    pub startup_timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ProviderSpec {
    Grok {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executable: Option<PathBuf>,
    },
    Custom {
        manifest: ProviderManifest,
    },
}

impl ProviderSpec {
    #[must_use]
    pub fn grok() -> Self {
        Self::Grok { executable: None }
    }

    pub fn manifest(&self) -> ProviderManifest {
        match self {
            Self::Grok { executable } => grok_manifest(executable.clone()),
            Self::Custom { manifest } => manifest.clone(),
        }
    }

    #[must_use]
    pub fn id(&self) -> ProviderId {
        match self {
            Self::Grok { .. } => ProviderId::new("grok"),
            Self::Custom { manifest } => manifest.id.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub provider: ProviderSpec,
    pub cwd: PathBuf,
    pub task: Task,
    pub permission_policy: PermissionPolicy,
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
    pub expected_version: VersionReq,
    pub required_capabilities: CapabilitySet,
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
