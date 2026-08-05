use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AuthenticateRequest, ClientCapabilities, ContentBlock, ContentChunk, CreateTerminalRequest,
    FileSystemCapabilities, InitializeRequest, KillTerminalRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReleaseTerminalRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason as AcpStopReason,
    TerminalOutputRequest, WaitForTerminalExitRequest, WriteTextFileRequest,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};

use crate::acp::{FileSystemHost, TerminalHost};
use crate::process::ProcessTreeOwner;
use crate::{
    FailureCode, OutputReceipt, PermissionPolicy, ProviderDriver, RunEventKind, RunFailure,
    RunStage, StopReason,
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
pub(super) struct OutputAccumulator {
    text: String,
    truncated: bool,
    event_count: u64,
}

impl OutputAccumulator {
    pub(super) fn push(&mut self, text: &str) {
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

    pub(super) fn snapshot(&self) -> OutputReceipt {
        OutputReceipt {
            text: self.text.clone(),
            truncated: self.truncated,
            event_count: self.event_count,
        }
    }
}

pub async fn run_one_shot(
    manifest: ProviderDriver,
    cwd: std::path::PathBuf,
    task: String,
    permission_policy: PermissionPolicy,
    stage: tokio::sync::watch::Sender<RunStage>,
    events: tokio::sync::mpsc::Sender<RunEventKind>,
) -> std::result::Result<OneShotAcpOutcome, AcpRunError> {
    let output = Arc::new(Mutex::new(OutputAccumulator::default()));
    let filesystem =
        FileSystemHost::new(cwd.clone(), permission_policy == PermissionPolicy::AllowAll).map_err(
            |error| AcpRunError {
                failure: RunFailure {
                    code: FailureCode::SessionCreateFailed,
                    stage: RunStage::SessionOpening,
                    retryable: false,
                    message: error.to_string(),
                },
                output: OutputReceipt::default(),
                process_started: false,
            },
        )?;
    let read_filesystem = filesystem.clone();
    let write_filesystem = filesystem;
    let terminal = TerminalHost::new(cwd.clone(), permission_policy == PermissionPolicy::AllowAll)
        .map_err(|error| AcpRunError {
            failure: RunFailure {
                code: FailureCode::SessionCreateFailed,
                stage: RunStage::SessionOpening,
                retryable: false,
                message: error.to_string(),
            },
            output: OutputReceipt::default(),
            process_started: false,
        })?;
    let create_terminal = terminal.clone();
    let output_terminal = terminal.clone();
    let wait_terminal = terminal.clone();
    let kill_terminal = terminal.clone();
    let release_terminal = terminal.clone();
    let notification_output = output.clone();
    let notification_events = events.clone();
    let owner = ProcessTreeOwner::new(manifest);
    let startup_timeout = owner.startup_timeout();
    let agent = owner.into_acp_agent();
    let callback_stage = stage.clone();

    let result = Client
        .builder()
        .name("acpxx")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                match notification.update {
                    SessionUpdate::AgentMessageChunk(ContentChunk {
                        content: ContentBlock::Text(text),
                        ..
                    }) => {
                        let _ = notification_events
                            .send(RunEventKind::OutputDelta {
                                content: text.text.clone(),
                            })
                            .await;
                        notification_output
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push(&text.text);
                    }
                    SessionUpdate::AgentThoughtChunk(ContentChunk {
                        content: ContentBlock::Text(text),
                        ..
                    }) => {
                        let _ = notification_events
                            .send(RunEventKind::ReasoningDelta { content: text.text })
                            .await;
                    }
                    SessionUpdate::ToolCall(tool) => {
                        let _ = notification_events
                            .send(RunEventKind::ToolStarted(crate::ToolEvent {
                                tool_call_id: tool.tool_call_id.to_string(),
                                title: Some(tool.title),
                                status: Some(format!("{:?}", tool.status).to_ascii_lowercase()),
                            }))
                            .await;
                    }
                    SessionUpdate::ToolCallUpdate(tool) => {
                        let event = crate::ToolEvent {
                            tool_call_id: tool.tool_call_id.to_string(),
                            title: tool.fields.title,
                            status: tool
                                .fields
                                .status
                                .map(|status| format!("{status:?}").to_ascii_lowercase()),
                        };
                        let kind = if matches!(
                            tool.fields.status,
                            Some(
                                agent_client_protocol::schema::v1::ToolCallStatus::Completed
                                    | agent_client_protocol::schema::v1::ToolCallStatus::Failed
                            )
                        ) {
                            RunEventKind::ToolCompleted(event)
                        } else {
                            RunEventKind::ToolUpdated(event)
                        };
                        let _ = notification_events.send(kind).await;
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                let _ = events
                    .send(RunEventKind::PermissionRequested(crate::PermissionEvent {
                        request_id: request.tool_call.tool_call_id.to_string(),
                        title: request.tool_call.fields.title.clone(),
                    }))
                    .await;
                match permission_policy {
                    PermissionPolicy::Deny => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                    PermissionPolicy::AllowAll => {
                        if let Some(option) = request.options.iter().find(|option| {
                            matches!(
                                option.kind,
                                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                            )
                        }) {
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
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, _cx| match read_filesystem
                .read(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, _cx| match write_filesystem
                .write(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateTerminalRequest, responder, _cx| match create_terminal
                .create(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest, responder, _cx| match output_terminal
                .output(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest, responder, _cx| match wait_terminal
                .wait_for_exit(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest, responder, _cx| match kill_terminal
                .kill(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest, responder, _cx| match release_terminal
                .release(request)
                .await
            {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
            callback_stage.send_replace(RunStage::AcpInitializing);
            let initialize = tokio::time::timeout(
                startup_timeout,
                connection
                    .send_request(
                        InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                            ClientCapabilities::new()
                                .fs(FileSystemCapabilities::new()
                                    .read_text_file(true)
                                    .write_text_file(true))
                                .terminal(permission_policy == PermissionPolicy::AllowAll),
                        ),
                    )
                    .block_task(),
            )
            .await
            .map_err(|_| {
                agent_client_protocol::Error::internal_error().data(format!(
                    "ACP initialize timed out after {startup_timeout:?}"
                ))
            })??;
            if initialize.protocol_version != ProtocolVersion::V1 {
                return Err(agent_client_protocol::Error::internal_error().data(format!(
                    "provider selected unsupported protocol version {:?}",
                    initialize.protocol_version
                )));
            }
            if let Some(method) = initialize.auth_methods.first() {
                callback_stage.send_replace(RunStage::Authenticating);
                tokio::time::timeout(
                    startup_timeout,
                    connection
                        .send_request(AuthenticateRequest::new(method.id().clone()))
                        .block_task(),
                )
                .await
                .map_err(|_| {
                    agent_client_protocol::Error::internal_error().data(format!(
                        "ACP authentication timed out after {startup_timeout:?}"
                    ))
                })??;
            }

            callback_stage.send_replace(RunStage::SessionOpening);
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

    terminal.shutdown().await;

    let output = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
                process_started: current_stage != RunStage::ProviderStarting,
            })
        }
    }
}

pub(super) fn map_stop_reason(reason: AcpStopReason) -> StopReason {
    match reason {
        AcpStopReason::EndTurn => StopReason::EndTurn,
        AcpStopReason::MaxTokens => StopReason::MaxTokens,
        AcpStopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
        AcpStopReason::Refusal => StopReason::Refusal,
        AcpStopReason::Cancelled => StopReason::Cancelled,
        _ => StopReason::Failed,
    }
}

pub(super) fn classify_acp_error(stage: RunStage, message: String) -> RunFailure {
    let lower = message.to_ascii_lowercase();
    let code = if stage == RunStage::ProviderStarting {
        FailureCode::ProviderSpawnFailed
    } else if lower.contains("incoming transport closed") || lower.contains("process exited") {
        FailureCode::ProviderCrashed
    } else if lower.contains("json") || lower.contains("parse") || lower.contains("protocol") {
        FailureCode::ProtocolCorruption
    } else {
        match stage {
            RunStage::AcpInitializing => FailureCode::AcpInitializeFailed,
            RunStage::Authenticating => FailureCode::AuthenticationFailed,
            RunStage::SessionOpening => FailureCode::SessionCreateFailed,
            RunStage::Prompting => FailureCode::PromptFailed,
            _ => FailureCode::ProviderCrashed,
        }
    };
    RunFailure {
        code,
        stage,
        retryable: matches!(code, FailureCode::ProviderCrashed),
        message: crate::security::redact_sensitive(&message),
    }
}
