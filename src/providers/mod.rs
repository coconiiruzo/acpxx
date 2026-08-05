mod claude;
mod codex;
mod cursor;
mod grok;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{AdmissionError, PermissionPolicy, ProviderId, Task};

pub use claude::{CLAUDE_ACP_TESTED_VERSION, CLAUDE_AGENT_SDK_TESTED_VERSION, claude_manifest};
pub use codex::{CODEX_ACP_TESTED_VERSION, CODEX_BUNDLED_TESTED_VERSION, codex_manifest};
pub use cursor::{CURSOR_TESTED_VERSION, cursor_manifest};
pub use grok::{GROK_TESTED_VERSION, grok_manifest};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpVersionPolicy {
    StableV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySet(pub Vec<String>);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum VersionProbe {
    Semver {
        args: Vec<String>,
        requirement: semver::VersionReq,
    },
    ExactOutput {
        args: Vec<String>,
        expected: String,
    },
}

impl VersionProbe {
    #[must_use]
    pub fn expected(&self) -> String {
        match self {
            Self::Semver { requirement, .. } => requirement.to_string(),
            Self::ExactOutput { expected, .. } => expected.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderManifest {
    pub id: ProviderId,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub version_probe: VersionProbe,
    pub protocol: AcpVersionPolicy,
    pub required_capabilities: CapabilitySet,
    pub allowed_env: Vec<String>,
    pub fixed_env: BTreeMap<String, String>,
    pub preferred_auth_method: Option<String>,
    pub startup_timeout: Duration,
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

    pub fn manifest(&self) -> std::result::Result<ProviderManifest, AdmissionError> {
        match self {
            Self::Grok { executable } => Ok(grok_manifest(executable.clone())),
            Self::Cursor { executable } => Ok(cursor_manifest(executable.clone())),
            Self::Codex { adapter } => Ok(codex_manifest(adapter.clone())),
            Self::Claude { adapter } => Ok(claude_manifest(adapter.clone())),
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
    pub expected_version: String,
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
