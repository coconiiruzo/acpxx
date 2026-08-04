use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, MessageId, ProviderId, RunHandle, RunId, RunStage, StopReason, TerminalRunState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    AdapterNotFound,
    AdapterVersionMismatch,
    AdapterSpawnFailed,
    AcpInitializeFailed,
    AcpVersionMismatch,
    SessionCreateFailed,
    PromptRejected,
    PermissionDenied,
    ProtocolCorruption,
    TransportClosed,
    ProviderCrashed,
    ContinuityLost,
    RunTimeout,
    InterruptTimeout,
    CleanupIncomplete,
    HostShutdown,
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
pub struct RunMetrics {
    pub total: Duration,
    pub provider_probe: Duration,
    pub model_and_tools: Duration,
    pub cleanup: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunReceipt {
    pub run_id: RunId,
    pub agent_id: AgentId,
    pub parent_run_id: Option<RunId>,
    pub provider: ProviderId,
    pub state: TerminalRunState,
    pub queued_at: SystemTime,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
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
