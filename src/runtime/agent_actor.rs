use std::sync::Arc;
use std::time::{Instant, SystemTime};

use tokio::sync::{mpsc, oneshot};

use crate::acp::{
    AcpMetricKind, AcpRunError, AcpSessionCommand, AcpSessionEvent, AcpSessionSetup,
    run_persistent_session,
};
use crate::runtime::{Registry, Scheduler, SchedulerPermit};
use crate::{
    AdmissionError, AgentMessage, AgentSnapshot, CleanupReceipt, Continuity, ContinuityLossReason,
    FailureCode, FollowupTask, InterruptReceipt, MessageId, MessageReceipt, OutputReceipt,
    ProcessDisposition, ProviderDriver, Result, RunFailure, RunHandle, RunId, RunMetrics,
    RunReceipt, RunSnapshot, RunStage, SpawnRequest, StopReason, TerminalRunState,
};

const MAILBOX_CAPACITY: usize = 1_024;
const SESSION_CHANNEL_CAPACITY: usize = 32;
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
const CANCEL_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(100);
const CANCEL_RETRY_COUNT: u8 = 3;

#[derive(Debug)]
pub enum AgentCommand {
    StartInitialRun {
        snapshot: Box<RunSnapshot>,
        request: SpawnRequest,
        manifest: Box<ProviderDriver>,
    },
    QueueMessage {
        message_id: MessageId,
        content: AgentMessage,
        response: oneshot::Sender<Result<MessageReceipt>>,
    },
    StartFollowup {
        after: RunId,
        task: FollowupTask,
        response: oneshot::Sender<Result<RunHandle>>,
    },
    Interrupt {
        run: RunHandle,
        response: oneshot::Sender<Result<InterruptReceipt>>,
    },
    SessionEvent(Box<AcpSessionEvent>),
    FollowupCapacity {
        run: RunHandle,
        content: String,
        permit: SchedulerPermit,
    },
    ForceInterrupt(RunHandle),
    Deadline(RunHandle),
    RetryCancel {
        run: RunHandle,
        remaining: u8,
    },
    IdleExpired {
        generation: u64,
    },
    Shutdown {
        response: oneshot::Sender<()>,
    },
}

struct ActiveRun {
    snapshot: RunSnapshot,
    provider: crate::ProviderId,
    started: Instant,
    metrics: RunMetrics,
    interrupt_stop_reason: Option<StopReason>,
}

struct SessionTasks {
    provider: tokio::task::JoinHandle<()>,
    event_forwarder: tokio::task::JoinHandle<()>,
}

impl SessionTasks {
    async fn abort_and_wait(mut self) {
        self.provider.abort();
        let _ = (&mut self.provider).await;
        let _ = self.event_forwarder.await;
    }

    async fn shutdown_and_wait(mut self) {
        if tokio::time::timeout(std::time::Duration::from_secs(3), &mut self.provider)
            .await
            .is_err()
        {
            self.provider.abort();
            let _ = self.provider.await;
        }
        let _ = self.event_forwarder.await;
    }
}

pub async fn run_agent_actor(
    mut commands: mpsc::UnboundedReceiver<AgentCommand>,
    command_sender: mpsc::UnboundedSender<AgentCommand>,
    registry: Arc<Registry>,
    scheduler: Scheduler,
    mut agent: AgentSnapshot,
    idle_ttl: std::time::Duration,
) {
    let mut mailbox = Vec::new();
    let mut mailbox_sequence = 0_u64;
    let mut session: Option<mpsc::Sender<AcpSessionCommand>> = None;
    let mut active: Option<ActiveRun> = None;
    let mut session_tasks: Option<SessionTasks> = None;
    let mut idle_generation = 0_u64;

    while let Some(command) = commands.recv().await {
        match command {
            AgentCommand::StartInitialRun {
                snapshot,
                request,
                manifest,
            } => {
                let snapshot = *snapshot;
                let provider = manifest.id;
                agent.active_run_id = Some(snapshot.run_id);
                registry.update_agent(agent.clone()).await;
                active = Some(ActiveRun {
                    snapshot: snapshot.clone(),
                    provider,
                    started: Instant::now(),
                    metrics: RunMetrics::default(),
                    interrupt_stop_reason: None,
                });
                if let Some(deadline) = request.task.deadline {
                    schedule_deadline(
                        command_sender.clone(),
                        RunHandle {
                            agent_id: snapshot.agent_id,
                            run_id: snapshot.run_id,
                        },
                        deadline,
                    );
                }
                let (session_sender, session_receiver) = mpsc::channel(SESSION_CHANNEL_CAPACITY);
                let (event_sender, mut event_receiver) = mpsc::channel::<AcpSessionEvent>(1_024);
                let forward = command_sender.clone();
                let event_forwarder = tokio::spawn(async move {
                    while let Some(event) = event_receiver.recv().await {
                        if forward
                            .send(AgentCommand::SessionEvent(Box::new(event)))
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                let session_for_task = session_sender.clone();
                let initial_run = RunHandle {
                    agent_id: snapshot.agent_id,
                    run_id: snapshot.run_id,
                };
                let scheduler = scheduler.clone();
                let provider = tokio::spawn(async move {
                    let Some(permit) = scheduler.acquire(provider).await else {
                        let _ = event_sender
                            .send(AcpSessionEvent::Exited(Some(RunFailure {
                                code: FailureCode::HostShutdown,
                                stage: RunStage::Admitted,
                                retryable: false,
                                message: "scheduler is shutting down".into(),
                            })))
                            .await;
                        return;
                    };
                    run_persistent_session(
                        AcpSessionSetup {
                            driver: *manifest,
                            cwd: request.cwd,
                            permission_policy: request.permission_policy,
                            assertions: request.assertions,
                            initial_run,
                            initial_permit: permit,
                        },
                        session_receiver,
                        event_sender,
                    )
                    .await;
                });
                session_tasks = Some(SessionTasks {
                    provider,
                    event_forwarder,
                });
                let _ = session_for_task
                    .send(AcpSessionCommand::Prompt {
                        run: initial_run,
                        content: request.task.content,
                        permit: None,
                    })
                    .await;
                session = Some(session_sender);
            }
            AgentCommand::QueueMessage {
                message_id,
                content,
                response,
            } => {
                if mailbox.len() == MAILBOX_CAPACITY {
                    let _ = response.send(Err(AdmissionError::InvalidRequest(
                        "agent mailbox is full".into(),
                    )
                    .into()));
                    continue;
                }
                mailbox_sequence += 1;
                mailbox.push((message_id, mailbox_sequence, content));
                registry
                    .set_mailbox_depth(agent.agent_id, mailbox.len())
                    .await;
                let _ = response.send(Ok(MessageReceipt {
                    message_id,
                    accepted_sequence: mailbox_sequence,
                }));
                if active.is_none() && agent.process_alive {
                    restart_idle_timer(&command_sender, idle_ttl, &mut idle_generation);
                }
            }
            AgentCommand::StartFollowup {
                after,
                task,
                response,
            } => {
                idle_generation = idle_generation.wrapping_add(1);
                let result = start_followup(
                    &registry,
                    FollowupState {
                        agent: &mut agent,
                        active: &mut active,
                        mailbox: &mut mailbox,
                    },
                    FollowupDispatch {
                        session_available: session.is_some(),
                        scheduler: scheduler.clone(),
                        command_sender: command_sender.clone(),
                    },
                    after,
                    task,
                )
                .await;
                registry
                    .set_mailbox_depth(agent.agent_id, mailbox.len())
                    .await;
                if result.is_err() && active.is_none() && agent.process_alive {
                    restart_idle_timer(&command_sender, idle_ttl, &mut idle_generation);
                }
                let _ = response.send(result);
            }
            AgentCommand::Interrupt { run, response } => {
                if active.as_ref().is_some_and(|current| {
                    current.snapshot.run_id == run.run_id
                        && current.snapshot.state == crate::RunState::Queued
                }) {
                    let current = active.take().expect("queued active Run must exist");
                    if agent.continuity.is_none() {
                        if let Some(tasks) = session_tasks.take() {
                            tasks.abort_and_wait().await;
                        }
                        session = None;
                        agent.process_alive = false;
                        agent.continuity =
                            Some(Continuity::Lost(ContinuityLossReason::ProviderExited));
                    }
                    finish_prompt(
                        &registry,
                        &mut agent,
                        current,
                        Ok(crate::acp::OneShotAcpOutcome {
                            stop_reason: StopReason::Cancelled,
                            output: OutputReceipt::default(),
                        }),
                    )
                    .await;
                    let _ = response.send(Ok(InterruptReceipt {
                        run,
                        requested: true,
                    }));
                    continue;
                }
                let result = interrupt_active(
                    &registry,
                    &mut active,
                    session.as_ref(),
                    run,
                    StopReason::Cancelled,
                )
                .await;
                if result.as_ref().is_ok_and(|receipt| receipt.requested) {
                    schedule_cancel_retry(command_sender.clone(), run, CANCEL_RETRY_COUNT);
                    let sender = command_sender.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(CANCEL_GRACE).await;
                        let _ = sender.send(AgentCommand::ForceInterrupt(run));
                    });
                }
                let _ = response.send(result);
            }
            AgentCommand::SessionEvent(event) => {
                let had_active_run = active.is_some();
                handle_session_event(&registry, &mut agent, &mut active, *event).await;
                if had_active_run && active.is_none() && agent.process_alive {
                    restart_idle_timer(&command_sender, idle_ttl, &mut idle_generation);
                }
            }
            AgentCommand::FollowupCapacity {
                run,
                content,
                permit,
            } => {
                if active
                    .as_ref()
                    .is_some_and(|current| current.snapshot.run_id == run.run_id)
                    && let Some(session) = session.as_ref()
                {
                    let _ = session
                        .send(AcpSessionCommand::Prompt {
                            run,
                            content,
                            permit: Some(permit),
                        })
                        .await;
                }
            }
            AgentCommand::ForceInterrupt(run) => {
                if active
                    .as_ref()
                    .is_some_and(|current| current.snapshot.run_id == run.run_id)
                {
                    if let Some(tasks) = session_tasks.take() {
                        tasks.abort_and_wait().await;
                    }
                    session = None;
                    agent.process_alive = false;
                    agent.continuity = Some(Continuity::Lost(ContinuityLossReason::ForcedKill));
                    if let Some(current) = active.take() {
                        finish_forced_interrupt(&registry, &mut agent, current).await;
                    }
                }
            }
            AgentCommand::Deadline(run) => {
                if active.as_ref().is_some_and(|current| {
                    current.snapshot.run_id == run.run_id
                        && current.snapshot.state == crate::RunState::Queued
                }) {
                    let mut current = active.take().expect("queued active Run must exist");
                    current.interrupt_stop_reason = Some(StopReason::DeadlineExceeded);
                    if agent.continuity.is_none() {
                        if let Some(tasks) = session_tasks.take() {
                            tasks.abort_and_wait().await;
                        }
                        session = None;
                        agent.process_alive = false;
                        agent.continuity =
                            Some(Continuity::Lost(ContinuityLossReason::ProviderExited));
                    }
                    finish_prompt(
                        &registry,
                        &mut agent,
                        current,
                        Ok(crate::acp::OneShotAcpOutcome {
                            stop_reason: StopReason::Cancelled,
                            output: OutputReceipt::default(),
                        }),
                    )
                    .await;
                } else if interrupt_active(
                    &registry,
                    &mut active,
                    session.as_ref(),
                    run,
                    StopReason::DeadlineExceeded,
                )
                .await
                .is_ok()
                {
                    schedule_cancel_retry(command_sender.clone(), run, CANCEL_RETRY_COUNT);
                    let sender = command_sender.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(CANCEL_GRACE).await;
                        let _ = sender.send(AgentCommand::ForceInterrupt(run));
                    });
                }
            }
            AgentCommand::RetryCancel { run, remaining } => {
                if active.as_ref().is_some_and(|current| {
                    current.snapshot.run_id == run.run_id && current.snapshot.interrupt_requested
                }) && let Some(session) = session.as_ref()
                {
                    let (response, _acknowledgement) = oneshot::channel();
                    let _ = session
                        .send(AcpSessionCommand::Cancel { run, response })
                        .await;
                    if remaining > 1 {
                        schedule_cancel_retry(command_sender.clone(), run, remaining - 1);
                    }
                }
            }
            AgentCommand::IdleExpired { generation } => {
                if generation != idle_generation || active.is_some() || !agent.process_alive {
                    continue;
                }
                if let Some(session) = session.take() {
                    let _ = session.send(AcpSessionCommand::Shutdown).await;
                }
                if let Some(tasks) = session_tasks.take() {
                    tasks.shutdown_and_wait().await;
                }
                agent.process_alive = false;
                agent.continuity = Some(Continuity::Lost(ContinuityLossReason::IdleExpired));
                registry.update_agent(agent.clone()).await;
            }
            AgentCommand::Shutdown { response } => {
                if let Some(session) = session.take() {
                    let _ = session.send(AcpSessionCommand::Shutdown).await;
                }
                if let Some(tasks) = session_tasks.take() {
                    tasks.shutdown_and_wait().await;
                }
                let process_started = agent.process_alive;
                agent.process_alive = false;
                agent.continuity = Some(Continuity::Lost(ContinuityLossReason::HostShutdown));
                if let Some(current) = active.take() {
                    finish_prompt(
                        &registry,
                        &mut agent,
                        current,
                        Err(AcpRunError {
                            failure: RunFailure {
                                code: FailureCode::HostShutdown,
                                stage: RunStage::CleaningUp,
                                retryable: false,
                                message: "broker shut down while the Run was active".into(),
                            },
                            output: OutputReceipt::default(),
                            process_started,
                        }),
                    )
                    .await;
                }
                registry.update_agent(agent.clone()).await;
                let _ = response.send(());
                break;
            }
        }
    }
    if let Some(session) = session {
        let _ = session.send(AcpSessionCommand::Shutdown).await;
    }
}

async fn finish_forced_interrupt(
    registry: &Registry,
    agent: &mut AgentSnapshot,
    mut current: ActiveRun,
) {
    let finished_at = SystemTime::now();
    let started_at = current.snapshot.started_at.unwrap_or(finished_at);
    if current.snapshot.state == crate::RunState::Queued {
        let _ = current.snapshot.start(started_at);
    }
    let stop_reason = current
        .interrupt_stop_reason
        .clone()
        .unwrap_or(StopReason::Cancelled);
    let _ = current.snapshot.finish(
        TerminalRunState::Interrupted,
        stop_reason.clone(),
        None,
        finished_at,
    );
    registry
        .finish_run(RunReceipt {
            run_id: current.snapshot.run_id,
            agent_id: current.snapshot.agent_id,
            parent_run_id: current.snapshot.parent_run_id,
            session_stamp: current.snapshot.session_stamp.clone(),
            provider: current.provider,
            provider_identity: current
                .snapshot
                .provider_identity
                .as_ref()
                .map(crate::ProviderExecutionIdentity::summary),
            state: TerminalRunState::Interrupted,
            queued_at: current.snapshot.queued_at,
            started_at,
            finished_at,
            completion_sequence: 0,
            stop_reason,
            failure: None,
            session_epoch: 0,
            output: OutputReceipt::default(),
            metrics: RunMetrics {
                total: current.started.elapsed(),
                ..current.metrics
            },
            cleanup: CleanupReceipt {
                complete: true,
                process: ProcessDisposition::Terminated,
            },
        })
        .await;
    agent.active_run_id = None;
    agent.latest_run_id = Some(current.snapshot.run_id);
    registry.update_agent(agent.clone()).await;
}

struct FollowupState<'a> {
    agent: &'a mut AgentSnapshot,
    active: &'a mut Option<ActiveRun>,
    mailbox: &'a mut Vec<(MessageId, u64, AgentMessage)>,
}

struct FollowupDispatch {
    session_available: bool,
    scheduler: Scheduler,
    command_sender: mpsc::UnboundedSender<AgentCommand>,
}

async fn start_followup(
    registry: &Registry,
    state: FollowupState<'_>,
    dispatch: FollowupDispatch,
    after: RunId,
    task: FollowupTask,
) -> Result<RunHandle> {
    let FollowupState {
        agent,
        active,
        mailbox,
    } = state;
    let FollowupDispatch {
        session_available,
        scheduler,
        command_sender,
    } = dispatch;
    if active.is_some() || agent.active_run_id.is_some() {
        return Err(AdmissionError::AgentBusy(agent.agent_id).into());
    }
    let Some(parent) = registry.current_terminal(after).await else {
        return Err(AdmissionError::StaleParent(after).into());
    };
    if agent.latest_run_id != Some(after) {
        return Err(AdmissionError::StaleParent(after).into());
    }
    let current_stamp = match &agent.continuity {
        Some(Continuity::Available(stamp)) => stamp,
        _ => return Err(AdmissionError::ContinuityAlreadyLost(agent.agent_id).into()),
    };
    if parent.session_stamp.as_ref() != Some(current_stamp) {
        agent.continuity = Some(Continuity::Lost(ContinuityLossReason::SessionChanged));
        return Err(AdmissionError::ContinuityAlreadyLost(agent.agent_id).into());
    }
    if parent.provider_identity.as_ref()
        != agent
            .provider_identity
            .as_ref()
            .map(crate::ProviderExecutionIdentity::summary)
            .as_ref()
    {
        agent.continuity = Some(Continuity::Lost(ContinuityLossReason::SessionChanged));
        return Err(AdmissionError::ContinuityAlreadyLost(agent.agent_id).into());
    }
    if !session_available {
        return Err(AdmissionError::ContinuityAlreadyLost(agent.agent_id).into());
    }
    let run_id = RunId::new();
    let mut snapshot = RunSnapshot::queued(run_id, agent.agent_id, SystemTime::now());
    snapshot.parent_run_id = Some(after);
    snapshot.session_stamp = Some(current_stamp.clone());
    snapshot.provider_identity = agent.provider_identity.clone();
    let handle = RunHandle {
        agent_id: agent.agent_id,
        run_id,
    };
    let mut content = String::new();
    for (_, _, message) in mailbox.iter() {
        content.push_str("[mailbox]\n");
        content.push_str(&message.content);
        content.push('\n');
    }
    content.push_str("[followup]\n");
    content.push_str(&task.content);
    registry.insert_run(snapshot.clone()).await;
    agent.active_run_id = Some(run_id);
    agent.latest_run_id = Some(run_id);
    registry.update_agent(agent.clone()).await;
    *active = Some(ActiveRun {
        snapshot,
        provider: agent.provider,
        started: Instant::now(),
        metrics: RunMetrics::default(),
        interrupt_stop_reason: None,
    });
    if let Some(deadline) = task.deadline {
        schedule_deadline(command_sender.clone(), handle, deadline);
    }
    mailbox.clear();
    let provider = agent.provider;
    tokio::spawn(async move {
        if let Some(permit) = scheduler.acquire(provider).await {
            let _ = command_sender.send(AgentCommand::FollowupCapacity {
                run: handle,
                content,
                permit,
            });
        }
    });
    Ok(handle)
}

async fn interrupt_active(
    registry: &Registry,
    active: &mut Option<ActiveRun>,
    session: Option<&mpsc::Sender<AcpSessionCommand>>,
    run: RunHandle,
    stop_reason: StopReason,
) -> Result<InterruptReceipt> {
    if let Some(receipt) = registry.current_terminal(run.run_id).await {
        if receipt.agent_id != run.agent_id {
            return Err(AdmissionError::HandleMismatch {
                agent_id: run.agent_id,
                run_id: run.run_id,
            }
            .into());
        }
        return Ok(InterruptReceipt {
            run,
            requested: false,
        });
    }
    let current = active
        .as_mut()
        .filter(|current| current.snapshot.run_id == run.run_id)
        .ok_or(AdmissionError::RunNotFound(run.run_id))?;
    current.snapshot.interrupt_requested = true;
    current.interrupt_stop_reason = Some(stop_reason);
    current.snapshot.stage = RunStage::Cancelling;
    registry.update_run(current.snapshot.clone()).await;
    let session = session.ok_or(AdmissionError::ContinuityAlreadyLost(run.agent_id))?;
    let (response, _acknowledgement) = oneshot::channel();
    session
        .send(AcpSessionCommand::Cancel { run, response })
        .await
        .map_err(|_| ControlErrorExt::continuity(run.agent_id))?;
    Ok(InterruptReceipt {
        run,
        requested: true,
    })
}

async fn handle_session_event(
    registry: &Registry,
    agent: &mut AgentSnapshot,
    active: &mut Option<ActiveRun>,
    event: AcpSessionEvent,
) {
    match event {
        AcpSessionEvent::Stage { run, stage } => {
            if let Some(current) = active
                .as_mut()
                .filter(|item| item.snapshot.run_id == run.run_id)
            {
                if current.snapshot.state == crate::RunState::Queued {
                    let _ = current.snapshot.start(SystemTime::now());
                }
                current.snapshot.stage = stage;
                agent.process_alive = true;
                registry.update_agent(agent.clone()).await;
                registry.update_run(current.snapshot.clone()).await;
            }
        }
        AcpSessionEvent::Ready {
            stamp,
            capabilities,
            identity,
        } => {
            agent.process_alive = true;
            agent.provider_capabilities = Some(capabilities);
            agent.provider_identity = Some(identity.clone());
            if let Some(current) = active.as_mut() {
                current.snapshot.session_stamp = Some(stamp.clone());
                current.snapshot.provider_identity = Some(identity);
                registry.update_run(current.snapshot.clone()).await;
            }
            agent.continuity = Some(Continuity::Available(stamp));
            registry.update_agent(agent.clone()).await;
        }
        AcpSessionEvent::IdentityObserved { identity } => {
            agent.provider_identity = Some(identity.clone());
            if let Some(current) = active.as_mut() {
                current.snapshot.provider_identity = Some(identity);
                registry.update_run(current.snapshot.clone()).await;
            }
            registry.update_agent(agent.clone()).await;
        }
        AcpSessionEvent::RunEvent {
            run,
            kind,
            provider_meta,
        } => registry.publish_event(run, kind, provider_meta),
        AcpSessionEvent::Metric {
            run,
            kind,
            duration,
        } => {
            if let Some(current) = active
                .as_mut()
                .filter(|item| item.snapshot.run_id == run.run_id)
            {
                match kind {
                    AcpMetricKind::ProviderProbe => current.metrics.provider_probe = duration,
                    AcpMetricKind::AdapterSpawn => current.metrics.adapter_spawn = duration,
                    AcpMetricKind::AcpInitialize => current.metrics.acp_initialize = duration,
                    AcpMetricKind::Authentication => current.metrics.authentication = duration,
                    AcpMetricKind::SessionNew => current.metrics.session_new = duration,
                    AcpMetricKind::FirstOutput => current.metrics.first_output = duration,
                    AcpMetricKind::ModelAndTools => current.metrics.model_and_tools = duration,
                    AcpMetricKind::Cleanup => current.metrics.cleanup = duration,
                }
            }
        }
        AcpSessionEvent::PromptFinished { run, outcome } => {
            if let Err(error) = &outcome
                && matches!(
                    error.failure.code,
                    FailureCode::ProviderCrashed | FailureCode::ProtocolCorruption
                )
            {
                agent.process_alive = false;
                agent.continuity = Some(Continuity::Lost(
                    if error.failure.code == FailureCode::ProtocolCorruption {
                        ContinuityLossReason::ProtocolCorruption
                    } else {
                        ContinuityLossReason::ProviderExited
                    },
                ));
            }
            if let Some(current) = active
                .take()
                .filter(|item| item.snapshot.run_id == run.run_id)
            {
                finish_prompt(registry, agent, current, outcome).await;
            }
        }
        AcpSessionEvent::Exited(failure) => {
            if failure.is_none()
                && !agent.process_alive
                && matches!(
                    agent.continuity,
                    Some(Continuity::Lost(
                        ContinuityLossReason::IdleExpired
                            | ContinuityLossReason::HostShutdown
                            | ContinuityLossReason::ForcedKill
                    ))
                )
            {
                return;
            }
            let process_started = !matches!(
                failure.as_ref().map(|item| item.code),
                Some(
                    FailureCode::ProviderSpawnFailed
                        | FailureCode::ProviderAssertionFailed
                        | FailureCode::ProviderArtifactChanged
                )
            );
            agent.process_alive = false;
            agent.continuity = Some(Continuity::Lost(
                match failure.as_ref().map(|item| item.code) {
                    Some(FailureCode::ProtocolCorruption) => {
                        ContinuityLossReason::ProtocolCorruption
                    }
                    _ => ContinuityLossReason::ProviderExited,
                },
            ));
            if let Some(current) = active.take() {
                let failure = failure.unwrap_or(RunFailure {
                    code: FailureCode::ProviderCrashed,
                    stage: current.snapshot.stage,
                    retryable: false,
                    message: "provider session exited before the Run completed".into(),
                });
                finish_prompt(
                    registry,
                    agent,
                    current,
                    Err(AcpRunError {
                        failure,
                        output: OutputReceipt::default(),
                        process_started,
                    }),
                )
                .await;
            }
            registry.update_agent(agent.clone()).await;
        }
    }
}

async fn finish_prompt(
    registry: &Registry,
    agent: &mut AgentSnapshot,
    mut current: ActiveRun,
    outcome: std::result::Result<crate::acp::OneShotAcpOutcome, AcpRunError>,
) {
    let finished_at = SystemTime::now();
    let started_at = current
        .snapshot
        .started_at
        .unwrap_or(current.snapshot.queued_at);
    let requested_stop_reason = current.interrupt_stop_reason.clone();
    let (state, stop_reason, failure, output, process) = match outcome {
        Ok(outcome) if outcome.stop_reason == StopReason::Cancelled => (
            TerminalRunState::Interrupted,
            requested_stop_reason.unwrap_or(StopReason::Cancelled),
            None,
            outcome.output,
            ProcessDisposition::Retained,
        ),
        Ok(outcome) => (
            TerminalRunState::Succeeded,
            outcome.stop_reason,
            None,
            outcome.output,
            ProcessDisposition::Retained,
        ),
        Err(error) => {
            let process = if !error.process_started {
                ProcessDisposition::NeverStarted
            } else if !agent.process_alive
                || matches!(
                    error.failure.code,
                    FailureCode::ProviderCrashed
                        | FailureCode::ProtocolCorruption
                        | FailureCode::HostShutdown
                )
            {
                ProcessDisposition::Terminated
            } else {
                ProcessDisposition::Retained
            };
            (
                TerminalRunState::Failed,
                StopReason::Failed,
                Some(error.failure),
                error.output,
                process,
            )
        }
    };
    if current.snapshot.state == crate::RunState::Queued {
        let _ = current.snapshot.start(started_at);
    }
    let _ = current
        .snapshot
        .finish(state, stop_reason.clone(), failure.clone(), finished_at);
    let session_epoch = match &agent.continuity {
        Some(Continuity::Available(stamp)) => stamp.transport_generation,
        _ => 0,
    };
    registry
        .finish_run(RunReceipt {
            run_id: current.snapshot.run_id,
            agent_id: current.snapshot.agent_id,
            parent_run_id: current.snapshot.parent_run_id,
            session_stamp: current.snapshot.session_stamp.clone(),
            provider: current.provider,
            provider_identity: current
                .snapshot
                .provider_identity
                .as_ref()
                .map(crate::ProviderExecutionIdentity::summary),
            state,
            queued_at: current.snapshot.queued_at,
            started_at,
            finished_at,
            completion_sequence: 0,
            stop_reason,
            failure,
            session_epoch,
            output,
            metrics: RunMetrics {
                total: current.started.elapsed(),
                ..current.metrics
            },
            cleanup: CleanupReceipt {
                complete: true,
                process,
            },
        })
        .await;
    agent.active_run_id = None;
    agent.latest_run_id = Some(current.snapshot.run_id);
    registry.update_agent(agent.clone()).await;
}

fn schedule_deadline(
    commands: mpsc::UnboundedSender<AgentCommand>,
    run: RunHandle,
    deadline: std::time::Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(deadline).await;
        let _ = commands.send(AgentCommand::Deadline(run));
    });
}

fn restart_idle_timer(
    commands: &mpsc::UnboundedSender<AgentCommand>,
    idle_ttl: std::time::Duration,
    generation: &mut u64,
) {
    *generation = generation.wrapping_add(1);
    let current = *generation;
    let commands = commands.clone();
    tokio::spawn(async move {
        tokio::time::sleep(idle_ttl).await;
        let _ = commands.send(AgentCommand::IdleExpired {
            generation: current,
        });
    });
}

fn schedule_cancel_retry(
    commands: mpsc::UnboundedSender<AgentCommand>,
    run: RunHandle,
    remaining: u8,
) {
    tokio::spawn(async move {
        tokio::time::sleep(CANCEL_RETRY_DELAY).await;
        let _ = commands.send(AgentCommand::RetryCancel { run, remaining });
    });
}

struct ControlErrorExt;

impl ControlErrorExt {
    fn continuity(agent_id: crate::AgentId) -> crate::ControlError {
        AdmissionError::ContinuityAlreadyLost(agent_id).into()
    }
}
