use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, MessageId, ProviderId, ProviderIdentitySummary, RunHandle, RunId, RunStage,
    SessionStamp, StopReason, TerminalRunState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    ProviderSpawnFailed,
    AcpInitializeFailed,
    AuthenticationFailed,
    SessionCreateFailed,
    PromptFailed,
    ProtocolCorruption,
    ProviderCrashed,
    ContinuityLost,
    DeadlineExceeded,
    CleanupIncomplete,
    HostShutdown,
    HostRestarted,
    ProviderAssertionFailed,
    ProviderArtifactChanged,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunFailure {
    pub code: FailureCode,
    pub stage: RunStage,
    pub retryable: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputReceipt {
    pub text: String,
    pub truncated: bool,
    pub event_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessDisposition {
    Terminated,
    Retained,
    NeverStarted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CleanupReceipt {
    pub complete: bool,
    pub process: ProcessDisposition,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunMetrics {
    pub total: Duration,
    pub provider_probe: Duration,
    pub adapter_spawn: Duration,
    pub acp_initialize: Duration,
    pub authentication: Duration,
    pub session_new: Duration,
    pub first_output: Duration,
    pub model_and_tools: Duration,
    pub cleanup: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunReceipt {
    pub run_id: RunId,
    pub agent_id: AgentId,
    pub parent_run_id: Option<RunId>,
    pub session_stamp: Option<SessionStamp>,
    pub provider: ProviderId,
    #[serde(default, alias = "provider_lock")]
    pub provider_identity: Option<ProviderIdentitySummary>,
    pub state: TerminalRunState,
    pub queued_at: SystemTime,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub completion_sequence: u64,
    pub stop_reason: StopReason,
    pub failure: Option<RunFailure>,
    pub session_epoch: u64,
    pub output: OutputReceipt,
    pub metrics: RunMetrics,
    pub cleanup: CleanupReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpawnReceipt {
    pub agent: crate::AgentHandle,
    pub run: RunHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageReceipt {
    pub message_id: MessageId,
    pub accepted_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InterruptReceipt {
    pub run: RunHandle,
    pub requested: bool,
}
