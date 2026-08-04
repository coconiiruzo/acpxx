use std::time::Duration;

use crate::{AgentId, InvalidRunTransition, RunId};

pub type Result<T> = std::result::Result<T, ControlError>;

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("agent not found: {0}")]
    AgentNotFound(AgentId),
    #[error("run not found: {0}")]
    RunNotFound(RunId),
    #[error("run handle belongs to a different agent")]
    HandleMismatch,
    #[error(transparent)]
    InvalidTransition(#[from] InvalidRunTransition),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("{operation} is reserved by the frozen API contract but is not implemented in Phase 1")]
    NotImplemented { operation: &'static str },
    #[error("wait timed out after {timeout:?}")]
    WaitTimeout { timeout: Duration },
    #[error("agent actor closed unexpectedly")]
    ActorClosed,
    #[error("internal error: {0}")]
    Internal(String),
}
