use std::time::Duration;

use crate::{AgentId, InvalidRunTransition, ProviderId, RunId};

pub type Result<T> = std::result::Result<T, ControlError>;

#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
    #[error("provider {provider} is not available in this build")]
    InvalidProvider { provider: ProviderId },
    #[error("invalid working directory {path}: {message}")]
    InvalidCwd { path: String, message: String },
    #[error("agent not found: {0}")]
    AgentNotFound(AgentId),
    #[error("run not found: {0}")]
    RunNotFound(RunId),
    #[error("run {run_id} does not belong to agent {agent_id}")]
    HandleMismatch { agent_id: AgentId, run_id: RunId },
    #[error("run {0} is not the latest terminal run for its agent")]
    StaleParent(RunId),
    #[error("agent is busy: {0}")]
    AgentBusy(AgentId),
    #[error("agent continuity is already lost: {0}")]
    ContinuityAlreadyLost(AgentId),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    #[error(transparent)]
    InvalidTransition(#[from] InvalidRunTransition),
    #[error(
        "{operation} is reserved by the frozen API contract but is not implemented in this build"
    )]
    NotImplemented { operation: &'static str },
    #[error("wait timed out after {timeout:?}")]
    WaitTimeout { timeout: Duration },
    #[error("agent actor closed unexpectedly")]
    ActorClosed,
    #[error("internal error: {0}")]
    Internal(String),
}
