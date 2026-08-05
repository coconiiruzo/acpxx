use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::runtime::{
    AgentCommand, Registry, RunEventStream, Scheduler, run_agent_actor, wait_for_terminal,
};
use crate::{
    AdmissionError, AgentHandle, AgentMessage, AgentSnapshot, ControlError, FollowupTask,
    InterruptReceipt, ListQuery, ListSnapshot, MessageReceipt, NonEmpty, ProviderSnapshot, Result,
    RunHandle, RunId, RunReceipt, RunSnapshot, SpawnReceipt, SpawnRequest, WaitOptions,
};

const DEFAULT_IDLE_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Clone)]
pub struct Broker {
    registry: Arc<Registry>,
    scheduler: Scheduler,
    idle_ttl: std::time::Duration,
}

impl Default for Broker {
    fn default() -> Self {
        Self::new(8)
    }
}

impl Broker {
    #[must_use]
    pub fn new(max_concurrent_agents: usize) -> Self {
        Self {
            registry: Arc::new(Registry::default()),
            scheduler: Scheduler::new(max_concurrent_agents),
            idle_ttl: DEFAULT_IDLE_TTL,
        }
    }

    #[must_use]
    pub fn with_idle_ttl(max_concurrent_agents: usize, idle_ttl: std::time::Duration) -> Self {
        Self {
            registry: Arc::new(Registry::default()),
            scheduler: Scheduler::new(max_concurrent_agents),
            idle_ttl: idle_ttl.max(std::time::Duration::from_millis(1)),
        }
    }

    #[must_use]
    pub fn with_provider_limits(
        max_concurrent_runs: usize,
        provider_limits: impl IntoIterator<Item = (crate::ProviderId, usize)>,
    ) -> Self {
        Self {
            registry: Arc::new(Registry::default()),
            scheduler: Scheduler::with_provider_limits(max_concurrent_runs, provider_limits),
            idle_ttl: DEFAULT_IDLE_TTL,
        }
    }

    pub async fn with_sqlite(
        max_concurrent_agents: usize,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        Self::with_sqlite_options(
            max_concurrent_agents,
            path,
            DEFAULT_IDLE_TTL,
            std::iter::empty(),
        )
        .await
    }

    pub async fn with_sqlite_and_idle_ttl(
        max_concurrent_agents: usize,
        path: impl AsRef<std::path::Path>,
        idle_ttl: std::time::Duration,
    ) -> Result<Self> {
        Self::with_sqlite_options(max_concurrent_agents, path, idle_ttl, std::iter::empty()).await
    }

    pub async fn with_sqlite_options(
        max_concurrent_runs: usize,
        path: impl AsRef<std::path::Path>,
        idle_ttl: std::time::Duration,
        provider_limits: impl IntoIterator<Item = (crate::ProviderId, usize)>,
    ) -> Result<Self> {
        let store = crate::storage::MetadataStore::open(path)?;
        let restored = store.restore_and_reconcile()?;
        let registry = Arc::new(Registry::new(Some(store)));
        for agent in restored.agents {
            registry.insert_historical_agent(agent).await;
        }
        for receipt in restored.terminal_runs {
            registry.insert_terminal_run(receipt).await;
        }
        Ok(Self {
            registry,
            scheduler: Scheduler::with_provider_limits(max_concurrent_runs, provider_limits),
            idle_ttl: idle_ttl.max(std::time::Duration::from_millis(1)),
        })
    }

    pub async fn spawn(&self, mut request: SpawnRequest) -> Result<SpawnReceipt> {
        if request.task.content.trim().is_empty() {
            return Err(
                AdmissionError::InvalidRequest("initial task must not be empty".into()).into(),
            );
        }
        request.cwd = canonicalize_cwd(&request.cwd)?;
        request
            .assertions
            .validate()
            .map_err(AdmissionError::InvalidRequest)?;
        let manifest = request.provider.driver()?;

        let agent_id = crate::AgentId::new();
        let run_id = RunId::new();
        let queued_at = SystemTime::now();
        let run = RunSnapshot::queued(run_id, agent_id, queued_at);
        let agent = AgentSnapshot {
            agent_id,
            provider: request.provider.id(),
            provider_identity: None,
            process_alive: false,
            continuity: None,
            provider_capabilities: None,
            broker_capabilities: crate::BrokerCapabilitySnapshot::default(),
            active_run_id: None,
            latest_run_id: Some(run_id),
            mailbox_depth: 0,
            display_name: None,
            display_path: None,
            cwd: request.cwd.clone(),
        };
        let (commands, receiver) = mpsc::unbounded_channel();

        self.registry.insert_run(run.clone()).await;
        self.registry
            .insert_agent(agent.clone(), commands.clone())
            .await;
        tokio::spawn(run_agent_actor(
            receiver,
            commands.clone(),
            self.registry.clone(),
            self.scheduler.clone(),
            agent,
            self.idle_ttl,
        ));
        commands
            .send(AgentCommand::StartInitialRun {
                snapshot: Box::new(run),
                request,
                manifest: Box::new(manifest),
            })
            .map_err(|_| ControlError::ActorClosed)?;

        Ok(SpawnReceipt {
            agent: AgentHandle { agent_id },
            run: RunHandle { agent_id, run_id },
        })
    }

    pub async fn send(&self, agent: AgentHandle, message: AgentMessage) -> Result<MessageReceipt> {
        if message.content.trim().is_empty() {
            return Err(AdmissionError::InvalidRequest("message must not be empty".into()).into());
        }
        if message.content.len() > 1024 * 1024 {
            return Err(AdmissionError::InvalidRequest(
                "message exceeds the 1 MiB mailbox-entry limit".into(),
            )
            .into());
        }
        let commands = self.live_agent_sender(agent.agent_id).await?;
        let message_id = crate::MessageId::new();
        let (response, receipt) = oneshot::channel();
        commands
            .send(AgentCommand::QueueMessage {
                message_id,
                content: message,
                response,
            })
            .map_err(|_| ControlError::ActorClosed)?;
        receipt.await.map_err(|_| ControlError::ActorClosed)?
    }

    pub async fn followup(
        &self,
        agent: AgentHandle,
        after: RunId,
        task: FollowupTask,
    ) -> Result<RunHandle> {
        if task.content.trim().is_empty() {
            return Err(
                AdmissionError::InvalidRequest("follow-up task must not be empty".into()).into(),
            );
        }
        let commands = self.live_agent_sender(agent.agent_id).await?;
        let (response, receipt) = oneshot::channel();
        commands
            .send(AgentCommand::StartFollowup {
                after,
                task,
                response,
            })
            .map_err(|_| ControlError::ActorClosed)?;
        receipt.await.map_err(|_| ControlError::ActorClosed)?
    }

    pub async fn interrupt(&self, run: RunHandle) -> Result<InterruptReceipt> {
        if let Some(receipt) = self.registry.current_terminal(run.run_id).await {
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
        let commands = self.live_agent_sender(run.agent_id).await?;
        let (response, receipt) = oneshot::channel();
        commands
            .send(AgentCommand::Interrupt { run, response })
            .map_err(|_| ControlError::ActorClosed)?;
        receipt.await.map_err(|_| ControlError::ActorClosed)?
    }

    /// Stops every Agent owned by this broker and waits for provider cleanup.
    ///
    /// This is a broker lifecycle barrier, not a public Agent control operation:
    /// it does not add a `close` operation to the Handle-first API.
    pub async fn shutdown(&self) -> Result<()> {
        let commands = self.registry.agent_command_senders().await;
        let mut acknowledgements = Vec::with_capacity(commands.len());
        for sender in commands {
            let (response, acknowledgement) = oneshot::channel();
            if sender.send(AgentCommand::Shutdown { response }).is_ok() {
                acknowledgements.push(acknowledgement);
            }
        }
        for acknowledgement in acknowledgements {
            acknowledgement
                .await
                .map_err(|_| ControlError::ActorClosed)?;
        }
        Ok(())
    }

    pub async fn list(&self, query: ListQuery) -> Result<ListSnapshot> {
        let mut agents = self.registry.agent_snapshots().await;
        let mut runs = self.registry.run_snapshots().await;
        agents.retain(|agent| {
            query.agent_id.is_none_or(|id| agent.agent_id == id)
                && query
                    .provider
                    .as_ref()
                    .is_none_or(|provider| &agent.provider == provider)
        });
        runs.retain(|run| {
            query.agent_id.is_none_or(|id| run.agent_id == id)
                && query.run_id.is_none_or(|id| run.run_id == id)
        });
        agents.sort_by_key(|agent| agent.agent_id);
        runs.sort_by_key(|run| run.run_id);

        let providers = crate::all_provider_drivers()
            .into_iter()
            .map(|manifest| ProviderSnapshot {
                id: manifest.id,
                protocol: manifest.protocol,
                driver_id: manifest.driver_id,
                driver_revision: manifest.driver_revision,
                required_capabilities: manifest.required_capabilities,
            })
            .collect();
        Ok(ListSnapshot {
            agents,
            runs,
            providers,
        })
    }

    pub async fn wait_run(&self, run: RunHandle, options: WaitOptions) -> Result<RunReceipt> {
        let receiver = self
            .registry
            .subscribe_run(run.run_id)
            .await
            .ok_or(AdmissionError::RunNotFound(run.run_id))?;
        match &*receiver.borrow() {
            crate::runtime::RunObservation::Active(snapshot)
                if snapshot.agent_id != run.agent_id =>
            {
                return Err(AdmissionError::HandleMismatch {
                    agent_id: run.agent_id,
                    run_id: run.run_id,
                }
                .into());
            }
            crate::runtime::RunObservation::Terminal(receipt)
                if receipt.agent_id != run.agent_id =>
            {
                return Err(AdmissionError::HandleMismatch {
                    agent_id: run.agent_id,
                    run_id: run.run_id,
                }
                .into());
            }
            _ => {}
        }
        Ok((*wait_for_terminal(receiver, options).await?).clone())
    }

    pub fn events(&self, run: RunHandle) -> Result<RunEventStream> {
        self.registry
            .subscribe_events(run)
            .map(|receiver| RunEventStream::new(run, receiver))
            .ok_or_else(|| AdmissionError::RunNotFound(run.run_id).into())
    }

    pub async fn wait_any(
        &self,
        runs: NonEmpty<RunHandle>,
        options: WaitOptions,
    ) -> Result<RunReceipt> {
        let handles: Vec<_> = runs.iter().copied().collect();
        let wait = async {
            let mut pending = FuturesUnordered::new();
            for handle in &handles {
                let receiver = self.checked_run_receiver(*handle).await?;
                pending.push(wait_for_terminal(receiver, WaitOptions::default()));
            }
            let first = pending.next().await.ok_or_else(|| {
                ControlError::Internal("wait_any received an empty handle set".into())
            })??;
            let mut completed = vec![first];
            for handle in &handles {
                if let Some(receipt) = self.registry.current_terminal(handle.run_id).await
                    && !completed.iter().any(|entry| entry.run_id == receipt.run_id)
                {
                    completed.push(receipt);
                }
            }
            completed
                .into_iter()
                .min_by_key(|receipt| receipt.completion_sequence)
                .map(|receipt| (*receipt).clone())
                .ok_or_else(|| ControlError::Internal("wait_any lost its completion".into()))
        };
        wait_with_timeout(wait, options).await
    }

    pub async fn wait_all(
        &self,
        runs: NonEmpty<RunHandle>,
        options: WaitOptions,
    ) -> Result<Vec<RunReceipt>> {
        let handles: Vec<_> = runs.iter().copied().collect();
        let wait = async {
            let mut receivers = Vec::with_capacity(handles.len());
            for handle in handles {
                receivers.push(self.checked_run_receiver(handle).await?);
            }
            let receipts = futures::future::try_join_all(
                receivers
                    .into_iter()
                    .map(|receiver| wait_for_terminal(receiver, WaitOptions::default())),
            )
            .await?;
            Ok(receipts
                .into_iter()
                .map(|receipt| (*receipt).clone())
                .collect())
        };
        wait_with_timeout(wait, options).await
    }

    async fn checked_run_receiver(
        &self,
        run: RunHandle,
    ) -> Result<tokio::sync::watch::Receiver<crate::runtime::RunObservation>> {
        let receiver = self
            .registry
            .subscribe_run(run.run_id)
            .await
            .ok_or(AdmissionError::RunNotFound(run.run_id))?;
        let agent_id = match &*receiver.borrow() {
            crate::runtime::RunObservation::Active(snapshot) => snapshot.agent_id,
            crate::runtime::RunObservation::Terminal(receipt) => receipt.agent_id,
        };
        if agent_id != run.agent_id {
            return Err(AdmissionError::HandleMismatch {
                agent_id: run.agent_id,
                run_id: run.run_id,
            }
            .into());
        }
        Ok(receiver)
    }

    async fn live_agent_sender(
        &self,
        agent_id: crate::AgentId,
    ) -> Result<mpsc::UnboundedSender<AgentCommand>> {
        if let Some(sender) = self.registry.agent_command_sender(agent_id).await {
            return Ok(sender);
        }
        if self.registry.agent_exists(agent_id).await {
            Err(AdmissionError::ContinuityAlreadyLost(agent_id).into())
        } else {
            Err(AdmissionError::AgentNotFound(agent_id).into())
        }
    }
}

async fn wait_with_timeout<T>(
    wait: impl std::future::Future<Output = Result<T>>,
    options: WaitOptions,
) -> Result<T> {
    match options.timeout {
        Some(timeout) => tokio::time::timeout(timeout, wait)
            .await
            .map_err(|_| ControlError::WaitTimeout { timeout })?,
        None => wait.await,
    }
}

fn canonicalize_cwd(cwd: &PathBuf) -> Result<PathBuf> {
    std::fs::canonicalize(cwd).map_err(|error| {
        AdmissionError::InvalidCwd {
            path: cwd.display().to_string(),
            message: error.to_string(),
        }
        .into()
    })
}
