use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason as AcpStopReason,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};

use crate::process::ProcessTreeOwner;
use crate::{
    FailureCode, OutputReceipt, PermissionPolicy, ProviderManifest, RunFailure, RunStage,
    StopReason,
};

const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct OneShotAcpOutcome {
    pub stop_reason: StopReason,
    pub output: OutputReceipt,
}

#[derive(Debug)]
pub struct AcpRunError {
    pub failure: RunFailure,
    pub output: OutputReceipt,
    pub process_started: bool,
}

#[derive(Default)]
struct OutputAccumulator {
    text: String,
    truncated: bool,
    event_count: u64,
}

impl OutputAccumulator {
    fn push(&mut self, text: &str) {
        self.event_count += 1;
        if self.text.len() >= OUTPUT_LIMIT {
            self.truncated = true;
            return;
        }
        let available = OUTPUT_LIMIT - self.text.len();
        if text.len() <= available {
            self.text.push_str(text);
            return;
        }
        let boundary = text
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= available)
            .last()
            .unwrap_or(0);
        self.text.push_str(&text[..boundary]);
        self.truncated = true;
    }

    fn snapshot(&self) -> OutputReceipt {
        OutputReceipt {
            text: self.text.clone(),
            truncated: self.truncated,
            event_count: self.event_count,
        }
    }
}

pub async fn run_one_shot(
    manifest: ProviderManifest,
    cwd: std::path::PathBuf,
    task: String,
    permission_policy: PermissionPolicy,
    stage: tokio::sync::watch::Sender<RunStage>,
) -> std::result::Result<OneShotAcpOutcome, AcpRunError> {
    let output = Arc::new(Mutex::new(OutputAccumulator::default()));
    let notification_output = output.clone();
    let owner = ProcessTreeOwner::new(manifest);
    let startup_timeout = owner.startup_timeout();
    let agent = owner.into_acp_agent();
    let callback_stage = stage.clone();

    let result = Client
        .builder()
        .name("acpxx")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(ContentChunk {
                    content: ContentBlock::Text(text),
                    ..
                }) = notification.update
                {
                    notification_output
                        .lock()
                        .expect("output accumulator lock poisoned")
                        .push(&text.text);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| match permission_policy {
                PermissionPolicy::Deny => responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                )),
                PermissionPolicy::AllowAll => {
                    if let Some(option) = request.options.first() {
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option.option_id.clone(),
                            )),
                        ))
                    } else {
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
            callback_stage.send_replace(RunStage::InitializingAcp);
            tokio::time::timeout(
                startup_timeout,
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task(),
            )
            .await
            .map_err(|_| {
                agent_client_protocol::Error::internal_error().data(format!(
                    "ACP initialize timed out after {startup_timeout:?}"
                ))
            })??;

            callback_stage.send_replace(RunStage::CreatingSession);
            let session = tokio::time::timeout(
                startup_timeout,
                connection
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task(),
            )
            .await
            .map_err(|_| {
                agent_client_protocol::Error::internal_error().data(format!(
                    "ACP session/new timed out after {startup_timeout:?}"
                ))
            })??;

            callback_stage.send_replace(RunStage::Prompting);
            let response = connection
                .send_request(PromptRequest::new(session.session_id, vec![task.into()]))
                .block_task()
                .await?;
            Ok(response.stop_reason)
        })
        .await;

    let output = output
        .lock()
        .expect("output accumulator lock poisoned")
        .snapshot();

    match result {
        Ok(reason) => Ok(OneShotAcpOutcome {
            stop_reason: map_stop_reason(reason),
            output,
        }),
        Err(error) => {
            let current_stage = *stage.borrow();
            let message = error.to_string();
            Err(AcpRunError {
                failure: classify_acp_error(current_stage, message),
                output,
                process_started: current_stage != RunStage::SpawningProvider,
            })
        }
    }
}

fn map_stop_reason(reason: AcpStopReason) -> StopReason {
    match reason {
        AcpStopReason::EndTurn => StopReason::EndTurn,
        AcpStopReason::MaxTokens => StopReason::MaxTokens,
        AcpStopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
        AcpStopReason::Refusal => StopReason::Refusal,
        AcpStopReason::Cancelled => StopReason::Cancelled,
        _ => StopReason::Failed,
    }
}

fn classify_acp_error(stage: RunStage, message: String) -> RunFailure {
    let lower = message.to_ascii_lowercase();
    let code = if stage == RunStage::SpawningProvider
        && (lower.contains("no such file") || lower.contains("not found"))
    {
        FailureCode::AdapterNotFound
    } else if stage == RunStage::SpawningProvider {
        FailureCode::AdapterSpawnFailed
    } else if lower.contains("incoming transport closed") || lower.contains("process exited") {
        FailureCode::ProviderCrashed
    } else if lower.contains("json") || lower.contains("parse") || lower.contains("protocol") {
        FailureCode::ProtocolCorruption
    } else {
        match stage {
            RunStage::InitializingAcp => FailureCode::AcpInitializeFailed,
            RunStage::CreatingSession => FailureCode::SessionCreateFailed,
            RunStage::Prompting => FailureCode::PromptRejected,
            _ => FailureCode::TransportClosed,
        }
    };
    RunFailure {
        code,
        stage,
        retryable: matches!(
            code,
            FailureCode::TransportClosed | FailureCode::ProviderCrashed
        ),
        message,
    }
}
