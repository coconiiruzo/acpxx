use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::{Semaphore, mpsc};

use crate::runtime::{AgentCommand, Registry, run_agent_actor, wait_for_terminal};
use crate::{
    AgentHandle, AgentMessage, AgentSnapshot, Continuity, ControlError, FollowupTask,
    InterruptReceipt, ListQuery, ListSnapshot, MessageReceipt, NonEmpty, ProviderSnapshot, Result,
    RunHandle, RunId, RunReceipt, RunSnapshot, SpawnReceipt, SpawnRequest, WaitOptions,
};

#[derive(Clone)]
pub struct Broker {
    registry: Arc<Registry>,
    semaphore: Arc<Semaphore>,
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
            semaphore: Arc::new(Semaphore::new(max_concurrent_agents.max(1))),
        }
    }

    pub async fn spawn(&self, mut request: SpawnRequest) -> Result<SpawnReceipt> {
        if request.task.content.trim().is_empty() {
            return Err(ControlError::InvalidRequest(
                "initial task must not be empty".into(),
            ));
        }
        request.cwd = canonicalize_cwd(&request.cwd)?;

        let agent_id = crate::AgentId::new();
        let run_id = RunId::new();
        let queued_at = SystemTime::now();
        let run = RunSnapshot::queued(run_id, agent_id, queued_at);
        let agent = AgentSnapshot {
            agent_id,
            provider: request.provider.id(),
            process_alive: false,
            continuity: Continuity::Available,
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
            self.registry.clone(),
            self.semaphore.clone(),
            agent,
        ));
        commands
            .send(AgentCommand::StartInitialRun {
                snapshot: run,
                request,
            })
            .map_err(|_| ControlError::ActorClosed)?;

        Ok(SpawnReceipt {
            agent: AgentHandle { agent_id },
            run: RunHandle { agent_id, run_id },
        })
    }

    pub async fn send(
        &self,
        _agent: AgentHandle,
        _message: AgentMessage,
    ) -> Result<MessageReceipt> {
        Err(ControlError::NotImplemented { operation: "send" })
    }

    pub async fn followup(&self, _parent: RunHandle, _task: FollowupTask) -> Result<RunHandle> {
        Err(ControlError::NotImplemented {
            operation: "followup",
        })
    }

    pub async fn interrupt(&self, _run: RunHandle) -> Result<InterruptReceipt> {
        Err(ControlError::NotImplemented {
            operation: "interrupt",
        })
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

        let grok = crate::grok_manifest(None);
        let providers = vec![ProviderSnapshot {
            id: grok.id,
            protocol: grok.protocol,
            expected_version: grok.expected_version,
            required_capabilities: grok.required_capabilities,
        }];
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
            .ok_or(ControlError::RunNotFound(run.run_id))?;
        match &*receiver.borrow() {
            crate::runtime::RunObservation::Active(snapshot)
                if snapshot.agent_id != run.agent_id =>
            {
                return Err(ControlError::HandleMismatch);
            }
            crate::runtime::RunObservation::Terminal(receipt)
                if receipt.agent_id != run.agent_id =>
            {
                return Err(ControlError::HandleMismatch);
            }
            _ => {}
        }
        Ok((*wait_for_terminal(receiver, options).await?).clone())
    }

    pub async fn wait_any(
        &self,
        _runs: NonEmpty<RunHandle>,
        _options: WaitOptions,
    ) -> Result<RunReceipt> {
        Err(ControlError::NotImplemented {
            operation: "wait_any",
        })
    }

    pub async fn wait_all(
        &self,
        _runs: NonEmpty<RunHandle>,
        _options: WaitOptions,
    ) -> Result<Vec<RunReceipt>> {
        Err(ControlError::NotImplemented {
            operation: "wait_all",
        })
    }
}

fn canonicalize_cwd(cwd: &PathBuf) -> Result<PathBuf> {
    std::fs::canonicalize(cwd).map_err(|error| {
        ControlError::InvalidRequest(format!(
            "cannot canonicalize cwd {}: {error}",
            cwd.display()
        ))
    })
}
