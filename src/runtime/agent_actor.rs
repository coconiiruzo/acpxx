use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use semver::Version;
use tokio::sync::{Semaphore, mpsc};

use crate::acp::run_one_shot;
use crate::runtime::Registry;
use crate::{
    AgentSnapshot, CleanupReceipt, Continuity, FailureCode, OutputReceipt, ProcessDisposition,
    ProviderManifest, RunFailure, RunMetrics, RunReceipt, RunSnapshot, RunStage, SpawnRequest,
    StopReason, TerminalRunState,
};

#[derive(Debug)]
pub enum AgentCommand {
    StartInitialRun {
        snapshot: RunSnapshot,
        request: SpawnRequest,
    },
}

pub async fn run_agent_actor(
    mut commands: mpsc::UnboundedReceiver<AgentCommand>,
    registry: Arc<Registry>,
    semaphore: Arc<Semaphore>,
    mut agent: AgentSnapshot,
) {
    while let Some(command) = commands.recv().await {
        match command {
            AgentCommand::StartInitialRun { snapshot, request } => {
                execute_initial_run(&registry, &semaphore, &mut agent, snapshot, request).await;
            }
        }
    }
}

async fn execute_initial_run(
    registry: &Arc<Registry>,
    semaphore: &Arc<Semaphore>,
    agent: &mut AgentSnapshot,
    mut snapshot: RunSnapshot,
    request: SpawnRequest,
) {
    snapshot.stage = RunStage::WaitingForCapacity;
    registry.update_run(snapshot.clone()).await;
    let permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            finish_without_start(
                registry,
                agent,
                snapshot,
                request.provider.manifest(),
                FailureCode::HostShutdown,
                "global scheduler is shutting down".into(),
            )
            .await;
            return;
        }
    };

    let started_at = SystemTime::now();
    let total_started = Instant::now();
    if let Err(error) = snapshot.start(started_at) {
        tracing::error!(%error, "run state invariant violated");
        return;
    }
    agent.active_run_id = Some(snapshot.run_id);
    registry.update_agent(agent.clone()).await;
    registry.update_run(snapshot.clone()).await;

    let manifest = request.provider.manifest();
    let probe_started = Instant::now();
    let probe_result = probe_provider_version(&manifest).await;
    let provider_probe = probe_started.elapsed();

    let model_started = Instant::now();
    let outcome = match probe_result {
        Ok(()) => {
            snapshot.stage = RunStage::SpawningProvider;
            agent.process_alive = true;
            registry.update_agent(agent.clone()).await;
            registry.update_run(snapshot.clone()).await;
            let (stage_sender, mut stage_receiver) =
                tokio::sync::watch::channel(RunStage::SpawningProvider);
            let mut run = Box::pin(run_one_shot(
                manifest.clone(),
                request.cwd,
                request.task.content,
                request.permission_policy,
                stage_sender,
            ));
            loop {
                tokio::select! {
                    result = &mut run => break result,
                    changed = stage_receiver.changed() => {
                        if changed.is_err() {
                            continue;
                        }
                        snapshot.stage = *stage_receiver.borrow_and_update();
                        registry.update_run(snapshot.clone()).await;
                    }
                }
            }
        }
        Err(failure) => Err(crate::acp::AcpRunError {
            failure,
            output: OutputReceipt::default(),
            process_started: false,
        }),
    };
    let model_and_tools = model_started.elapsed();
    snapshot.stage = RunStage::CleaningUp;
    registry.update_run(snapshot.clone()).await;
    let cleanup_started = Instant::now();
    drop(permit);
    let cleanup = cleanup_started.elapsed();
    let finished_at = SystemTime::now();

    let (state, stop_reason, failure, output, process) = match outcome {
        Ok(outcome) => {
            let state = if outcome.stop_reason == StopReason::Cancelled {
                TerminalRunState::Interrupted
            } else {
                TerminalRunState::Succeeded
            };
            (
                state,
                outcome.stop_reason,
                None,
                outcome.output,
                ProcessDisposition::Terminated,
            )
        }
        Err(error) => (
            TerminalRunState::Failed,
            StopReason::Failed,
            Some(error.failure),
            error.output,
            if error.process_started {
                ProcessDisposition::Terminated
            } else {
                ProcessDisposition::NeverStarted
            },
        ),
    };

    if let Err(error) = snapshot.finish(state, stop_reason.clone(), failure.clone(), finished_at) {
        tracing::error!(%error, "run state invariant violated");
        return;
    }
    let receipt = RunReceipt {
        run_id: snapshot.run_id,
        agent_id: snapshot.agent_id,
        parent_run_id: None,
        provider: manifest.id,
        state,
        queued_at: snapshot.queued_at,
        started_at,
        finished_at,
        stop_reason,
        failure,
        session_epoch: 1,
        output,
        metrics: RunMetrics {
            total: total_started.elapsed(),
            provider_probe,
            model_and_tools,
            cleanup,
        },
        cleanup: CleanupReceipt {
            complete: true,
            process,
        },
    };

    agent.process_alive = false;
    agent.continuity = Continuity::Lost;
    agent.active_run_id = None;
    agent.latest_run_id = Some(snapshot.run_id);
    registry.update_agent(agent.clone()).await;
    registry.finish_run(receipt).await;
}

async fn probe_provider_version(
    manifest: &ProviderManifest,
) -> std::result::Result<(), RunFailure> {
    let output = tokio::process::Command::new(&manifest.command)
        .args(&manifest.version_args)
        .output()
        .await
        .map_err(|error| RunFailure {
            code: if error.kind() == std::io::ErrorKind::NotFound {
                FailureCode::AdapterNotFound
            } else {
                FailureCode::AdapterSpawnFailed
            },
            stage: RunStage::ProbingProvider,
            retryable: false,
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(RunFailure {
            code: FailureCode::AdapterSpawnFailed,
            stage: RunStage::ProbingProvider,
            retryable: false,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text
        .split_whitespace()
        .find_map(|word| {
            Version::parse(word.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric()
                    && character != '.'
                    && character != '-'
                    && character != '+'
            }))
            .ok()
        })
        .ok_or_else(|| RunFailure {
            code: FailureCode::AdapterVersionMismatch,
            stage: RunStage::ProbingProvider,
            retryable: false,
            message: format!("could not parse provider version from {text:?}"),
        })?;
    if !manifest.expected_version.matches(&version) {
        return Err(RunFailure {
            code: FailureCode::AdapterVersionMismatch,
            stage: RunStage::ProbingProvider,
            retryable: false,
            message: format!(
                "provider version {version} does not satisfy tested requirement {}",
                manifest.expected_version
            ),
        });
    }
    Ok(())
}

async fn finish_without_start(
    registry: &Registry,
    agent: &mut AgentSnapshot,
    mut snapshot: RunSnapshot,
    manifest: ProviderManifest,
    code: FailureCode,
    message: String,
) {
    let started_at = SystemTime::now();
    if snapshot.start(started_at).is_err() {
        return;
    }
    let finished_at = SystemTime::now();
    let failure = RunFailure {
        code,
        stage: RunStage::WaitingForCapacity,
        retryable: false,
        message,
    };
    if snapshot
        .finish(
            TerminalRunState::Failed,
            StopReason::Failed,
            Some(failure.clone()),
            finished_at,
        )
        .is_err()
    {
        return;
    }
    agent.active_run_id = None;
    agent.latest_run_id = Some(snapshot.run_id);
    agent.continuity = Continuity::Lost;
    registry.update_agent(agent.clone()).await;
    registry
        .finish_run(RunReceipt {
            run_id: snapshot.run_id,
            agent_id: snapshot.agent_id,
            parent_run_id: None,
            provider: manifest.id,
            state: TerminalRunState::Failed,
            queued_at: snapshot.queued_at,
            started_at,
            finished_at,
            stop_reason: StopReason::Failed,
            failure: Some(failure),
            session_epoch: 1,
            output: OutputReceipt::default(),
            metrics: RunMetrics {
                total: Duration::ZERO,
                ..RunMetrics::default()
            },
            cleanup: CleanupReceipt {
                complete: true,
                process: ProcessDisposition::NeverStarted,
            },
        })
        .await;
}
