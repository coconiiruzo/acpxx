use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }
    };
}

uuid_id!(AgentId);
uuid_id!(RunId);
uuid_id!(MessageId);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AgentHandle {
    pub agent_id: AgentId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RunHandle {
    pub agent_id: AgentId,
    pub run_id: RunId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Codex,
    Claude,
    Grok,
    Cursor,
}

impl ProviderId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Grok => "grok",
            Self::Cursor => "cursor",
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProviderId {
    type Err = ParseProviderIdError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "grok" => Ok(Self::Grok),
            "cursor" => Ok(Self::Cursor),
            _ => Err(ParseProviderIdError(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unsupported provider: {0}")]
pub struct ParseProviderIdError(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl RunState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Interrupted)
    }

    pub fn transition(self, next: Self) -> std::result::Result<Self, InvalidRunTransition> {
        match (self, next) {
            (Self::Queued, Self::Running)
            | (Self::Running, Self::Succeeded)
            | (Self::Running, Self::Failed)
            | (Self::Running, Self::Interrupted) => Ok(next),
            _ => Err(InvalidRunTransition {
                from: self,
                to: next,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid run transition: {from:?} -> {to:?}")]
pub struct InvalidRunTransition {
    pub from: RunState,
    pub to: RunState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalRunState {
    Succeeded,
    Failed,
    Interrupted,
}

impl From<TerminalRunState> for RunState {
    fn from(value: TerminalRunState) -> Self {
        match value {
            TerminalRunState::Succeeded => Self::Succeeded,
            TerminalRunState::Failed => Self::Failed,
            TerminalRunState::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStage {
    Admitted,
    ProviderStarting,
    AcpInitializing,
    Authenticating,
    SessionOpening,
    Prompting,
    Cancelling,
    CleaningUp,
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionStamp {
    pub provider_session_id: String,
    pub transport_generation: u64,
    pub adapter_instance_id: Uuid,
    pub provider_profile_fingerprint: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityLossReason {
    ProviderExited,
    TransportClosed,
    SessionChanged,
    AdapterRestarted,
    IdleExpired,
    ForcedKill,
    HostShutdown,
    HostRestarted,
    ProtocolCorruption,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
pub enum Continuity {
    Available(SessionStamp),
    Lost(ContinuityLossReason),
}

/// Capabilities implemented by the frozen agentmux v1 surface.
///
/// This is deliberately separate from `provider_capabilities`: an ACP provider
/// may advertise optional protocol features that the broker records but does
/// not expose or emulate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrokerCapabilitySnapshot {
    pub text_prompt: bool,
    pub event_stream: bool,
    pub permission_response: bool,
    pub filesystem_host: bool,
    pub terminal_host: bool,
    pub image_prompt: bool,
    pub audio_prompt: bool,
    pub embedded_context: bool,
    pub mcp_servers: bool,
    pub session_load: bool,
    pub session_resume: bool,
}

impl Default for BrokerCapabilitySnapshot {
    fn default() -> Self {
        Self {
            text_prompt: true,
            event_stream: true,
            permission_response: true,
            filesystem_host: true,
            terminal_host: true,
            image_prompt: false,
            audio_prompt: false,
            embedded_context: false,
            mcp_servers: false,
            session_load: false,
            session_resume: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub agent_id: AgentId,
    pub parent_run_id: Option<RunId>,
    pub session_stamp: Option<SessionStamp>,
    #[serde(default)]
    pub provider_identity: Option<crate::ProviderExecutionIdentity>,
    pub state: RunState,
    pub stage: RunStage,
    pub interrupt_requested: bool,
    pub stop_reason: Option<StopReason>,
    pub failure: Option<crate::receipt::RunFailure>,
    pub queued_at: SystemTime,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
}

impl RunSnapshot {
    #[must_use]
    pub fn queued(run_id: RunId, agent_id: AgentId, queued_at: SystemTime) -> Self {
        Self {
            run_id,
            agent_id,
            parent_run_id: None,
            session_stamp: None,
            provider_identity: None,
            state: RunState::Queued,
            stage: RunStage::Admitted,
            interrupt_requested: false,
            stop_reason: None,
            failure: None,
            queued_at,
            started_at: None,
            finished_at: None,
        }
    }

    pub fn start(&mut self, at: SystemTime) -> std::result::Result<(), InvalidRunTransition> {
        self.state = self.state.transition(RunState::Running)?;
        self.stage = RunStage::ProviderStarting;
        self.started_at = Some(at);
        Ok(())
    }

    pub fn finish(
        &mut self,
        terminal: TerminalRunState,
        stop_reason: StopReason,
        failure: Option<crate::receipt::RunFailure>,
        at: SystemTime,
    ) -> std::result::Result<(), InvalidRunTransition> {
        self.state = self.state.transition(terminal.into())?;
        self.stage = RunStage::Terminal;
        self.stop_reason = Some(stop_reason);
        self.failure = failure;
        self.finished_at = Some(at);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub agent_id: AgentId,
    pub provider: ProviderId,
    #[serde(default)]
    pub provider_identity: Option<crate::ProviderExecutionIdentity>,
    pub process_alive: bool,
    pub continuity: Option<Continuity>,
    pub provider_capabilities: Option<Value>,
    #[serde(default)]
    pub broker_capabilities: BrokerCapabilitySnapshot,
    pub active_run_id: Option<RunId>,
    pub latest_run_id: Option<RunId>,
    pub mailbox_depth: usize,
    pub display_name: Option<String>,
    pub display_path: Option<String>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    pub seq: u64,
    pub run_id: RunId,
    pub agent_id: AgentId,
    pub timestamp: SystemTime,
    pub kind: RunEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_meta: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum RunEventKind {
    OutputDelta { content: String },
    ReasoningDelta { content: String },
    ToolStarted(ToolEvent),
    ToolUpdated(ToolEvent),
    ToolCompleted(ToolEvent),
    PermissionRequested(PermissionEvent),
    Diagnostic(DiagnosticEvent),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolEvent {
    pub tool_call_id: String,
    pub title: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PermissionEvent {
    pub request_id: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<std::time::Duration>,
}

impl Task {
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            deadline: None,
        }
    }

    #[must_use]
    pub const fn with_deadline(mut self, deadline: std::time::Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FollowupTask {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<std::time::Duration>,
}

impl FollowupTask {
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            deadline: None,
        }
    }

    #[must_use]
    pub const fn with_deadline(mut self, deadline: std::time::Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    Deny,
    AllowAll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRetention {
    OneShot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    DeadlineExceeded,
    Failed,
}

#[derive(Clone, Debug)]
pub struct NonEmpty<T> {
    pub head: T,
    pub tail: Vec<T>,
}

impl<T> NonEmpty<T> {
    #[must_use]
    pub fn new(head: T) -> Self {
        Self {
            head,
            tail: Vec::new(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.head).chain(&self.tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_canonical_run_transitions_are_allowed() {
        assert_eq!(
            RunState::Queued.transition(RunState::Running),
            Ok(RunState::Running)
        );
        assert!(RunState::Queued.transition(RunState::Succeeded).is_err());
        assert!(RunState::Running.transition(RunState::Queued).is_err());
        assert!(RunState::Succeeded.transition(RunState::Running).is_err());
    }

    #[test]
    fn ids_are_uuid_v7_and_distinct_from_display_metadata() {
        let first = AgentId::new();
        let second = AgentId::new();
        assert_ne!(first, second);
        assert_eq!(first.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn provider_ids_are_a_closed_four_value_set() {
        let providers = [
            ProviderId::Codex,
            ProviderId::Claude,
            ProviderId::Grok,
            ProviderId::Cursor,
        ];
        assert_eq!(
            providers.map(ProviderId::as_str),
            ["codex", "claude", "grok", "cursor"]
        );
        assert!("custom".parse::<ProviderId>().is_err());
    }
}
