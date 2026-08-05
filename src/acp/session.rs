use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AuthenticateRequest, CancelNotification, ClientCapabilities, ContentBlock, ContentChunk,
    CreateTerminalRequest, FileSystemCapabilities, InitializeRequest, KillTerminalRequest,
    NewSessionRequest, PermissionOptionKind, PromptRequest, ReadTextFileRequest,
    ReleaseTerminalRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    TerminalOutputRequest, WaitForTerminalExitRequest, WriteTextFileRequest,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::client::{OutputAccumulator, classify_acp_error, map_stop_reason};
use crate::acp::{AcpRunError, FileSystemHost, OneShotAcpOutcome, TerminalHost};
use crate::process::ProcessTreeOwner;
use crate::runtime::SchedulerPermit;
use crate::{
    FailureCode, OutputReceipt, PermissionEvent, PermissionPolicy, ProviderManifest, RunEventKind,
    RunFailure, RunHandle, RunStage, SessionStamp, ToolEvent,
};

#[derive(Debug)]
pub(crate) enum AcpSessionCommand {
    Prompt {
        run: RunHandle,
        content: String,
        permit: Option<SchedulerPermit>,
    },
    Cancel {
        run: RunHandle,
        response: oneshot::Sender<std::result::Result<(), String>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub(crate) enum AcpSessionEvent {
    Stage {
        run: RunHandle,
        stage: RunStage,
    },
    Ready {
        stamp: SessionStamp,
        capabilities: serde_json::Value,
    },
    RunEvent {
        run: RunHandle,
        kind: RunEventKind,
        provider_meta: Option<serde_json::Value>,
    },
    Metric {
        run: RunHandle,
        kind: AcpMetricKind,
        duration: Duration,
    },
    PromptFinished {
        run: RunHandle,
        outcome: std::result::Result<OneShotAcpOutcome, AcpRunError>,
    },
    Exited(Option<RunFailure>),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AcpMetricKind {
    ProviderProbe,
    AdapterSpawn,
    AcpInitialize,
    Authentication,
    SessionNew,
    FirstOutput,
    ModelAndTools,
    Cleanup,
}

#[derive(Default)]
struct TurnState {
    run: Option<RunHandle>,
    output: OutputAccumulator,
    prompt_started: Option<Instant>,
    first_output_recorded: bool,
}

pub(crate) async fn run_persistent_session(
    manifest: ProviderManifest,
    cwd: std::path::PathBuf,
    permission_policy: PermissionPolicy,
    initial_run: RunHandle,
    initial_permit: SchedulerPermit,
    mut commands: mpsc::Receiver<AcpSessionCommand>,
    events: mpsc::Sender<AcpSessionEvent>,
) {
    let provider_probe_started = Instant::now();
    if let Err(failure) = probe_provider_version(&manifest).await {
        let _ = events.send(AcpSessionEvent::Exited(Some(failure))).await;
        return;
    }
    let _ = events
        .send(AcpSessionEvent::Metric {
            run: initial_run,
            kind: AcpMetricKind::ProviderProbe,
            duration: provider_probe_started.elapsed(),
        })
        .await;
    let turn = Arc::new(Mutex::new(TurnState::default()));
    let notification_turn = turn.clone();
    let notification_events = events.clone();
    let permission_turn = turn.clone();
    let permission_events = events.clone();
    let filesystem =
        match FileSystemHost::new(cwd.clone(), permission_policy == PermissionPolicy::AllowAll) {
            Ok(filesystem) => filesystem,
            Err(error) => {
                let _ = events
                    .send(AcpSessionEvent::Exited(Some(RunFailure {
                        code: FailureCode::SessionCreateFailed,
                        stage: RunStage::SessionOpening,
                        retryable: false,
                        message: error.to_string(),
                    })))
                    .await;
                return;
            }
        };
    let read_filesystem = filesystem.clone();
    let write_filesystem = filesystem;
    let terminal =
        match TerminalHost::new(cwd.clone(), permission_policy == PermissionPolicy::AllowAll) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = events
                    .send(AcpSessionEvent::Exited(Some(RunFailure {
                        code: FailureCode::SessionCreateFailed,
                        stage: RunStage::SessionOpening,
                        retryable: false,
                        message: error.to_string(),
                    })))
                    .await;
                return;
            }
        };
    let create_terminal = terminal.clone();
    let output_terminal = terminal.clone();
    let wait_terminal = terminal.clone();
    let kill_terminal = terminal.clone();
    let release_terminal = terminal.clone();
    let cancel_terminals = terminal.clone();
    let startup_timeout = manifest.startup_timeout;
    let profile_fingerprint = format!(
        "{}:{}:{}",
        manifest.id,
        manifest.command.display(),
        manifest.version_probe.expected()
    );
    let preferred_auth_method = manifest.preferred_auth_method.clone();
    let agent = ProcessTreeOwner::new(manifest).into_acp_agent();
    let stage = Arc::new(Mutex::new(RunStage::ProviderStarting));
    let callback_stage = stage.clone();
    let callback_events = events.clone();

    let adapter_spawn_started = Instant::now();
    let result = Client
        .builder()
        .name("agentmux")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                let run = notification_turn
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .run;
                let Some(run) = run else { return Ok(()) };
                let provider_meta = provider_metadata(&notification.update);
                let kind = match notification.update {
                    SessionUpdate::AgentMessageChunk(ContentChunk {
                        content: ContentBlock::Text(text),
                        ..
                    }) => {
                        let first_output = {
                            let mut state = notification_turn
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            state.output.push(&text.text);
                            if state.first_output_recorded {
                                None
                            } else {
                                state.first_output_recorded = true;
                                state.prompt_started.map(|started| started.elapsed())
                            }
                        };
                        if let Some(duration) = first_output {
                            let _ = notification_events
                                .send(AcpSessionEvent::Metric {
                                    run,
                                    kind: AcpMetricKind::FirstOutput,
                                    duration,
                                })
                                .await;
                        }
                        Some(RunEventKind::OutputDelta { content: text.text })
                    }
                    SessionUpdate::AgentThoughtChunk(ContentChunk {
                        content: ContentBlock::Text(text),
                        ..
                    }) => Some(RunEventKind::ReasoningDelta { content: text.text }),
                    SessionUpdate::ToolCall(tool) => {
                        Some(RunEventKind::ToolStarted(ToolEvent {
                            tool_call_id: tool.tool_call_id.to_string(),
                            title: Some(tool.title),
                            status: Some(format!("{:?}", tool.status).to_ascii_lowercase()),
                        }))
                    }
                    SessionUpdate::ToolCallUpdate(tool) => {
                        let terminal = matches!(
                            tool.fields.status,
                            Some(
                                agent_client_protocol::schema::v1::ToolCallStatus::Completed
                                    | agent_client_protocol::schema::v1::ToolCallStatus::Failed
                            )
                        );
                        let event = ToolEvent {
                            tool_call_id: tool.tool_call_id.to_string(),
                            title: tool.fields.title,
                            status: tool
                                .fields
                                .status
                                .map(|value| format!("{value:?}").to_ascii_lowercase()),
                        };
                        Some(if terminal {
                            RunEventKind::ToolCompleted(event)
                        } else {
                            RunEventKind::ToolUpdated(event)
                        })
                    }
                    _ => None,
                };
                if let Some(kind) = kind {
                    let _ = notification_events
                        .send(AcpSessionEvent::RunEvent {
                            run,
                            kind,
                            provider_meta,
                        })
                        .await;
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                let run = permission_turn
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .run;
                if let Some(run) = run {
                    let provider_meta = provider_metadata(&request);
                    let _ = permission_events
                        .send(AcpSessionEvent::RunEvent {
                            run,
                            kind: RunEventKind::PermissionRequested(PermissionEvent {
                                request_id: request.tool_call.tool_call_id.to_string(),
                                title: request.tool_call.fields.title.clone(),
                            }),
                            provider_meta,
                        })
                        .await;
                }
                match permission_policy {
                    PermissionPolicy::Deny => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                    PermissionPolicy::AllowAll => match request.options.iter().find(|option| {
                        matches!(
                            option.kind,
                            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                        )
                    }) {
                        Some(option) => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                option.option_id.clone(),
                            )),
                        )),
                        None => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    },
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, _cx| {
                match read_filesystem.read(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, _cx| {
                match write_filesystem.write(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateTerminalRequest, responder, _cx| {
                match create_terminal.create(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest, responder, _cx| {
                match output_terminal.output(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest, responder, _cx| {
                match wait_terminal.wait_for_exit(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest, responder, _cx| {
                match kill_terminal.kill(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest, responder, _cx| {
                match release_terminal.release(request).await {
                    Ok(response) => responder.respond(response),
                    Err(error) => responder.respond_with_error(error),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
            let _ = callback_events
                .send(AcpSessionEvent::Metric {
                    run: initial_run,
                    kind: AcpMetricKind::AdapterSpawn,
                    duration: adapter_spawn_started.elapsed(),
                })
                .await;
            set_stage(&callback_stage, &callback_events, initial_run, RunStage::AcpInitializing)
                .await;
            let initialize_started = Instant::now();
            let initialize = tokio::time::timeout(
                startup_timeout,
                connection
                    .send_request(
                        InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                            ClientCapabilities::new()
                                .fs(
                                    FileSystemCapabilities::new()
                                        .read_text_file(true)
                                        .write_text_file(true),
                                )
                                .terminal(permission_policy == PermissionPolicy::AllowAll),
                        ),
                    )
                    .block_task(),
            )
            .await
            .map_err(|_| agent_client_protocol::Error::internal_error().data("initialize timeout"))??;
            let _ = callback_events
                .send(AcpSessionEvent::Metric {
                    run: initial_run,
                    kind: AcpMetricKind::AcpInitialize,
                    duration: initialize_started.elapsed(),
                })
                .await;
            if initialize.protocol_version != ProtocolVersion::V1 {
                return Err(agent_client_protocol::Error::internal_error()
                    .data("provider selected a non-v1 protocol"));
            }
            let auth_method = match preferred_auth_method.as_deref() {
                Some(preferred) => {
                    set_stage(
                        &callback_stage,
                        &callback_events,
                        initial_run,
                        RunStage::Authenticating,
                    )
                    .await;
                    Some(initialize
                        .auth_methods
                        .iter()
                        .find(|method| method.id().0.as_ref() == preferred)
                        .ok_or_else(|| {
                            agent_client_protocol::Error::internal_error().data(format!(
                                "provider did not advertise required auth method {preferred}"
                            ))
                        })?)
                }
                None => initialize.auth_methods.first(),
            };
            if let Some(method) = auth_method {
                if preferred_auth_method.is_none() {
                    set_stage(
                        &callback_stage,
                        &callback_events,
                        initial_run,
                        RunStage::Authenticating,
                    )
                    .await;
                }
                let authentication_started = Instant::now();
                tokio::time::timeout(
                    startup_timeout,
                    connection
                        .send_request(AuthenticateRequest::new(method.id().clone()))
                        .block_task(),
                )
                .await
                .map_err(|_| {
                    agent_client_protocol::Error::internal_error().data("authentication timeout")
                })??;
                let _ = callback_events
                    .send(AcpSessionEvent::Metric {
                        run: initial_run,
                        kind: AcpMetricKind::Authentication,
                        duration: authentication_started.elapsed(),
                    })
                    .await;
            }
            set_stage(&callback_stage, &callback_events, initial_run, RunStage::SessionOpening)
                .await;
            let session_started = Instant::now();
            let session = tokio::time::timeout(
                startup_timeout,
                connection
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task(),
            )
            .await
            .map_err(|_| {
                agent_client_protocol::Error::internal_error().data("session/new timeout")
            })??;
            let _ = callback_events
                .send(AcpSessionEvent::Metric {
                    run: initial_run,
                    kind: AcpMetricKind::SessionNew,
                    duration: session_started.elapsed(),
                })
                .await;
            let session_id = session.session_id;
            let capabilities = serde_json::to_value(&initialize.agent_capabilities)
                .unwrap_or(serde_json::Value::Null);
            let _ = callback_events
                .send(AcpSessionEvent::Ready {
                    stamp: SessionStamp {
                        provider_session_id: session_id.to_string(),
                        transport_generation: 1,
                        adapter_instance_id: Uuid::now_v7(),
                        provider_profile_fingerprint: profile_fingerprint,
                    },
                    capabilities,
                })
                .await;

            let mut initial_permit = Some(initial_permit);
            while let Some(command) = commands.recv().await {
                match command {
                    AcpSessionCommand::Prompt {
                        run,
                        content,
                        permit,
                    } => {
                        let _permit = permit;
                        {
                            let mut state = turn
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            state.run = Some(run);
                            state.output = OutputAccumulator::default();
                            state.prompt_started = Some(Instant::now());
                            state.first_output_recorded = false;
                        }
                        let mut prompt = Box::pin(
                            connection
                                .send_request(PromptRequest::new(
                                    session_id.clone(),
                                    vec![content.into()],
                                ))
                                .block_task(),
                        );
                        set_stage(&callback_stage, &callback_events, run, RunStage::Prompting).await;
                        let outcome = loop {
                            tokio::select! {
                                result = &mut prompt => {
                                    let output = turn
                                        .lock()
                                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                                        .output
                                        .snapshot();
                                    break match result {
                                        Ok(response) => Ok(OneShotAcpOutcome {
                                            stop_reason: map_stop_reason(response.stop_reason),
                                            output,
                                        }),
                                        Err(error) => Err(AcpRunError {
                                            failure: classify_acp_error(
                                                RunStage::Prompting,
                                                error.to_string(),
                                            ),
                                            output,
                                            process_started: true,
                                        }),
                                    };
                                }
                                nested = commands.recv() => match nested {
                                    Some(AcpSessionCommand::Cancel { run: target, response }) if target == run => {
                                        let result = connection
                                            .send_notification(CancelNotification::new(session_id.clone()))
                                            .map_err(|error| error.to_string());
                                        cancel_terminals.kill_all().await;
                                        let _ = response.send(result);
                                    }
                                    Some(AcpSessionCommand::Shutdown) | None => {
                                        let _ = connection.send_notification(
                                            CancelNotification::new(session_id.clone()),
                                        );
                                        cancel_terminals.kill_all().await;
                                        return Ok(());
                                    }
                                    Some(AcpSessionCommand::Cancel { response, .. }) => {
                                        let _ = response.send(Err("run is not active".into()));
                                    }
                                    Some(AcpSessionCommand::Prompt { run: rejected, .. }) => {
                                        let _ = callback_events.send(AcpSessionEvent::PromptFinished {
                                            run: rejected,
                                            outcome: Err(AcpRunError {
                                                failure: RunFailure {
                                                    code: FailureCode::PromptFailed,
                                                    stage: RunStage::Prompting,
                                                    retryable: false,
                                                    message: "session already has an active prompt".into(),
                                                },
                                                output: OutputReceipt::default(),
                                                process_started: true,
                                            }),
                                        }).await;
                                    }
                                }
                            }
                        };
                        let cleanup_started = Instant::now();
                        let model_and_tools = {
                            let mut state = turn
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            state.run = None;
                            state
                                .prompt_started
                                .take()
                                .map(|started| started.elapsed())
                                .unwrap_or_default()
                        };
                        let _ = callback_events
                            .send(AcpSessionEvent::Metric {
                                run,
                                kind: AcpMetricKind::ModelAndTools,
                                duration: model_and_tools,
                            })
                            .await;
                        let _ = callback_events
                            .send(AcpSessionEvent::Metric {
                                run,
                                kind: AcpMetricKind::Cleanup,
                                duration: cleanup_started.elapsed(),
                            })
                            .await;
                        let _ = callback_events
                            .send(AcpSessionEvent::PromptFinished { run, outcome })
                            .await;
                        initial_permit.take();
                    }
                    AcpSessionCommand::Cancel { response, .. } => {
                        let _ = response.send(Err("no active run".into()));
                    }
                    AcpSessionCommand::Shutdown => return Ok(()),
                }
            }
            Ok(())
        })
        .await;

    terminal.shutdown().await;

    let failure = result.err().map(|error| {
        let current = *stage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        classify_acp_error(current, error.to_string())
    });
    let _ = events.send(AcpSessionEvent::Exited(failure)).await;
}

pub(crate) async fn probe_provider_version(
    manifest: &ProviderManifest,
) -> std::result::Result<(), RunFailure> {
    let args = match &manifest.version_probe {
        crate::VersionProbe::Semver { args, .. }
        | crate::VersionProbe::ExactOutput { args, .. } => args,
    };
    let mut command = tokio::process::Command::new(&manifest.command);
    command.args(args).env_clear();
    for name in &manifest.allowed_env {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, value) in &manifest.fixed_env {
        command.env(name, value);
    }
    let output = command.output().await.map_err(|error| RunFailure {
        code: FailureCode::ProviderSpawnFailed,
        stage: RunStage::ProviderStarting,
        retryable: false,
        message: error.to_string(),
    })?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let matches = match &manifest.version_probe {
        crate::VersionProbe::Semver { requirement, .. } => text
            .split_whitespace()
            .find_map(|word| {
                semver::Version::parse(word.trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric()
                        && character != '.'
                        && character != '-'
                        && character != '+'
                }))
                .ok()
            })
            .is_some_and(|version| requirement.matches(&version)),
        crate::VersionProbe::ExactOutput { expected, .. } => &text == expected,
    };
    if !output.status.success() || !matches {
        return Err(RunFailure {
            code: FailureCode::ProviderSpawnFailed,
            stage: RunStage::ProviderStarting,
            retryable: false,
            message: format!(
                "provider version output {text:?} does not match {}",
                manifest.version_probe.expected()
            ),
        });
    }
    Ok(())
}

async fn set_stage(
    stage: &Mutex<RunStage>,
    events: &mpsc::Sender<AcpSessionEvent>,
    run: RunHandle,
    value: RunStage,
) {
    *stage
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
    let _ = events
        .send(AcpSessionEvent::Stage { run, stage: value })
        .await;
}

const MAX_PROVIDER_META_BYTES: usize = 64 * 1024;

fn provider_metadata(value: &impl serde::Serialize) -> Option<serde_json::Value> {
    let value = serde_json::to_value(value).ok()?;
    let mut extensions = Vec::new();
    let mut terminal_ids = Vec::new();
    collect_provider_metadata(&value, &mut extensions, &mut terminal_ids);
    if extensions.is_empty() && terminal_ids.is_empty() {
        return None;
    }
    let mut metadata = serde_json::json!({
        "extensions": extensions,
        "terminal_ids": terminal_ids,
    });
    crate::security::redact_json_sensitive(&mut metadata);
    if serde_json::to_vec(&metadata).ok()?.len() > MAX_PROVIDER_META_BYTES {
        return Some(serde_json::json!({ "truncated": true }));
    }
    Some(metadata)
}

fn collect_provider_metadata(
    value: &serde_json::Value,
    extensions: &mut Vec<serde_json::Value>,
    terminal_ids: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(extension) = object.get("_meta")
                && extension.as_object().is_some_and(|value| !value.is_empty())
            {
                extensions.push(extension.clone());
            }
            if let Some(terminal_id) = object.get("terminalId").and_then(|value| value.as_str())
                && !terminal_ids.iter().any(|existing| existing == terminal_id)
            {
                terminal_ids.push(terminal_id.to_owned());
            }
            for child in object.values() {
                collect_provider_metadata(child, extensions, terminal_ids);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_provider_metadata(child, extensions, terminal_ids);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Scheduler;
    use crate::{AcpVersionPolicy, CapabilitySet, ProviderId, VersionProbe};

    #[tokio::test]
    async fn version_probe_receives_only_allowlisted_environment_variables() {
        let manifest = ProviderManifest {
            id: ProviderId::Grok,
            command: "/bin/sh".into(),
            args: Vec::new(),
            version_probe: VersionProbe::ExactOutput {
                args: vec![
                    "-c".into(),
                    "printf '%s' \"${PATH-unset}:${CARGO_MANIFEST_DIR-unset}:${AGENTMUX_FIXED-unset}\""
                        .into(),
                ],
                expected: format!("{}:unset:fixed", std::env::var("PATH").unwrap()),
            },
            protocol: AcpVersionPolicy::StableV1,
            required_capabilities: CapabilitySet(Vec::new()),
            allowed_env: vec!["PATH".into(), "AGENTMUX_FIXED".into()],
            fixed_env: [("AGENTMUX_FIXED".into(), "fixed".into())]
                .into_iter()
                .collect(),
            preferred_auth_method: None,
            startup_timeout: Duration::from_secs(1),
        };

        probe_provider_version(&manifest).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn authentication_timeout_is_classified_and_session_task_exits() {
        use std::os::unix::fs::symlink;

        let fixture =
            std::env::temp_dir().join(format!("agentmux-mock-grok-auth_hang-{}", Uuid::now_v7()));
        symlink(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mock_acp_agent.py"),
            &fixture,
        )
        .unwrap();
        let manifest = ProviderManifest {
            id: ProviderId::Grok,
            command: fixture.clone(),
            args: Vec::new(),
            version_probe: VersionProbe::ExactOutput {
                args: vec!["--version".into()],
                expected: "grok 0.2.118".into(),
            },
            protocol: AcpVersionPolicy::StableV1,
            required_capabilities: CapabilitySet(Vec::new()),
            allowed_env: vec!["PATH".into()],
            fixed_env: Default::default(),
            preferred_auth_method: Some("cached_token".into()),
            startup_timeout: Duration::from_millis(50),
        };
        let run = RunHandle {
            agent_id: crate::AgentId::new(),
            run_id: crate::RunId::new(),
        };
        let scheduler = Scheduler::new(1);
        let permit = scheduler.acquire(ProviderId::Grok).await.unwrap();
        let (commands, command_receiver) = mpsc::channel(2);
        commands
            .send(AcpSessionCommand::Prompt {
                run,
                content: "never reaches prompt".into(),
                permit: None,
            })
            .await
            .unwrap();
        let (events, mut event_receiver) = mpsc::channel(32);
        let session = tokio::spawn(run_persistent_session(
            manifest,
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf(),
            PermissionPolicy::Deny,
            run,
            permit,
            command_receiver,
            events,
        ));
        let failure = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(AcpSessionEvent::Exited(Some(failure))) = event_receiver.recv().await {
                    break failure;
                }
            }
        })
        .await
        .expect("authentication timeout did not terminate the session");
        assert_eq!(failure.code, FailureCode::AuthenticationFailed);
        assert_eq!(failure.stage, RunStage::Authenticating);
        session.await.unwrap();
        let _ = std::fs::remove_file(fixture);
    }
}
