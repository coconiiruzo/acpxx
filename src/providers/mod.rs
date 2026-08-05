mod claude;
mod codex;
mod cursor;
mod grok;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{AdmissionError, DriverId, PermissionPolicy, ProviderAssertions, ProviderId, Task};

pub use claude::claude_driver;
pub use codex::codex_driver;
pub use cursor::cursor_driver;
pub use grok::grok_driver;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpProtocolPolicy {
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
    pub protocol: AcpProtocolPolicy,
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
#[serde(deny_unknown_fields)]
pub struct SpawnRequest {
    pub provider: ProviderSpec,
    pub cwd: PathBuf,
    pub task: Task,
    pub permission_policy: PermissionPolicy,
    #[serde(default)]
    pub assertions: ProviderAssertions,
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
    pub protocol: AcpProtocolPolicy,
    pub driver_id: DriverId,
    pub driver_revision: u32,
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
