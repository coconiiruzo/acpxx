use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc, watch};

use crate::runtime::AgentCommand;
use crate::{AgentId, AgentSnapshot, RunId, RunReceipt, RunSnapshot};

#[derive(Clone, Debug)]
pub enum RunObservation {
    Active(RunSnapshot),
    Terminal(Arc<RunReceipt>),
}

#[derive(Clone)]
pub struct AgentEntry {
    pub snapshot: AgentSnapshot,
    pub _commands: mpsc::UnboundedSender<AgentCommand>,
}

#[derive(Clone)]
pub struct RunEntry {
    pub sender: watch::Sender<RunObservation>,
}

#[derive(Default)]
pub struct Registry {
    agents: RwLock<HashMap<AgentId, AgentEntry>>,
    runs: RwLock<HashMap<RunId, RunEntry>>,
}

impl Registry {
    pub async fn insert_agent(
        &self,
        snapshot: AgentSnapshot,
        commands: mpsc::UnboundedSender<AgentCommand>,
    ) {
        self.agents.write().await.insert(
            snapshot.agent_id,
            AgentEntry {
                snapshot,
                _commands: commands,
            },
        );
    }

    pub async fn insert_run(&self, snapshot: RunSnapshot) {
        let run_id = snapshot.run_id;
        let (sender, _) = watch::channel(RunObservation::Active(snapshot));
        self.runs.write().await.insert(run_id, RunEntry { sender });
    }

    pub async fn subscribe_run(&self, run_id: RunId) -> Option<watch::Receiver<RunObservation>> {
        self.runs
            .read()
            .await
            .get(&run_id)
            .map(|entry| entry.sender.subscribe())
    }

    pub async fn update_run(&self, snapshot: RunSnapshot) {
        if let Some(entry) = self.runs.read().await.get(&snapshot.run_id) {
            entry.sender.send_replace(RunObservation::Active(snapshot));
        }
    }

    pub async fn finish_run(&self, receipt: RunReceipt) {
        if let Some(entry) = self.runs.read().await.get(&receipt.run_id) {
            entry
                .sender
                .send_replace(RunObservation::Terminal(Arc::new(receipt)));
        }
    }

    pub async fn update_agent(&self, snapshot: AgentSnapshot) {
        if let Some(entry) = self.agents.write().await.get_mut(&snapshot.agent_id) {
            entry.snapshot = snapshot;
        }
    }

    pub async fn agent_snapshots(&self) -> Vec<AgentSnapshot> {
        self.agents
            .read()
            .await
            .values()
            .map(|entry| entry.snapshot.clone())
            .collect()
    }

    pub async fn run_snapshots(&self) -> Vec<RunSnapshot> {
        self.runs
            .read()
            .await
            .values()
            .map(|entry| match &*entry.sender.borrow() {
                RunObservation::Active(snapshot) => snapshot.clone(),
                RunObservation::Terminal(receipt) => RunSnapshot {
                    run_id: receipt.run_id,
                    agent_id: receipt.agent_id,
                    parent_run_id: receipt.parent_run_id,
                    state: receipt.state.into(),
                    stage: crate::RunStage::Finished,
                    interrupt_requested: false,
                    stop_reason: Some(receipt.stop_reason.clone()),
                    failure: receipt.failure.clone(),
                    queued_at: receipt.queued_at,
                    started_at: Some(receipt.started_at),
                    finished_at: Some(receipt.finished_at),
                },
            })
            .collect()
    }
}
