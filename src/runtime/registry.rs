use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as SyncRwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use tokio::sync::{RwLock, broadcast, mpsc, watch};

use crate::runtime::AgentCommand;
use crate::{
    AgentId, AgentSnapshot, RunEvent, RunEventKind, RunHandle, RunId, RunReceipt, RunSnapshot,
};

const EVENT_CHANNEL_CAPACITY: usize = 1_024;

#[derive(Clone, Debug)]
pub enum RunObservation {
    Active(Box<RunSnapshot>),
    Terminal(Arc<RunReceipt>),
}

#[derive(Clone)]
pub struct AgentEntry {
    pub snapshot: AgentSnapshot,
    pub commands: Option<mpsc::UnboundedSender<AgentCommand>>,
}

#[derive(Clone)]
pub struct RunEntry {
    pub sender: watch::Sender<RunObservation>,
}

pub struct Registry {
    agents: RwLock<HashMap<AgentId, AgentEntry>>,
    runs: RwLock<HashMap<RunId, RunEntry>>,
    events: SyncRwLock<HashMap<RunId, EventEntry>>,
    global_sequence: AtomicU64,
    store: Option<crate::storage::MetadataStore>,
}

#[derive(Clone)]
struct EventEntry {
    agent_id: AgentId,
    sender: broadcast::Sender<RunEvent>,
}

impl Registry {
    pub fn new(store: Option<crate::storage::MetadataStore>) -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            runs: RwLock::new(HashMap::new()),
            events: SyncRwLock::new(HashMap::new()),
            global_sequence: AtomicU64::new(0),
            store,
        }
    }

    pub async fn insert_agent(
        &self,
        snapshot: AgentSnapshot,
        commands: mpsc::UnboundedSender<AgentCommand>,
    ) {
        self.agents.write().await.insert(
            snapshot.agent_id,
            AgentEntry {
                snapshot: snapshot.clone(),
                commands: Some(commands),
            },
        );
        self.persist_agent(&snapshot);
    }

    pub async fn insert_historical_agent(&self, snapshot: AgentSnapshot) {
        self.agents.write().await.insert(
            snapshot.agent_id,
            AgentEntry {
                snapshot,
                commands: None,
            },
        );
    }

    pub async fn insert_run(&self, snapshot: RunSnapshot) {
        self.persist_run_snapshot(&snapshot);
        let run_id = snapshot.run_id;
        let (event_sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        self.events
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                run_id,
                EventEntry {
                    agent_id: snapshot.agent_id,
                    sender: event_sender,
                },
            );
        let (sender, _) = watch::channel(RunObservation::Active(Box::new(snapshot)));
        self.runs.write().await.insert(run_id, RunEntry { sender });
    }

    pub async fn insert_terminal_run(&self, receipt: RunReceipt) {
        let run_id = receipt.run_id;
        let (event_sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        self.events
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                run_id,
                EventEntry {
                    agent_id: receipt.agent_id,
                    sender: event_sender,
                },
            );
        self.global_sequence
            .fetch_max(receipt.completion_sequence, Ordering::SeqCst);
        let (sender, _) = watch::channel(RunObservation::Terminal(Arc::new(receipt)));
        self.runs.write().await.insert(run_id, RunEntry { sender });
    }

    pub fn subscribe_events(&self, run: RunHandle) -> Option<broadcast::Receiver<RunEvent>> {
        let events = self
            .events
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = events.get(&run.run_id)?;
        (entry.agent_id == run.agent_id).then(|| entry.sender.subscribe())
    }

    pub fn publish_event(
        &self,
        run: RunHandle,
        kind: RunEventKind,
        provider_meta: Option<serde_json::Value>,
    ) {
        let seq = self.global_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RunEvent {
            seq,
            run_id: run.run_id,
            agent_id: run.agent_id,
            timestamp: SystemTime::now(),
            kind,
            provider_meta,
        };
        if let Some(entry) = self
            .events
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&run.run_id)
        {
            let _ = entry.sender.send(event);
        }
    }

    pub async fn subscribe_run(&self, run_id: RunId) -> Option<watch::Receiver<RunObservation>> {
        self.runs
            .read()
            .await
            .get(&run_id)
            .map(|entry| entry.sender.subscribe())
    }

    pub async fn update_run(&self, snapshot: RunSnapshot) {
        self.persist_run_snapshot(&snapshot);
        if let Some(entry) = self.runs.read().await.get(&snapshot.run_id) {
            entry
                .sender
                .send_replace(RunObservation::Active(Box::new(snapshot)));
        }
    }

    pub async fn finish_run(&self, mut receipt: RunReceipt) {
        receipt.completion_sequence = self.global_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        self.persist_receipt(&receipt);
        if let Some(entry) = self.runs.read().await.get(&receipt.run_id) {
            entry
                .sender
                .send_replace(RunObservation::Terminal(Arc::new(receipt)));
        }
    }

    pub async fn current_terminal(&self, run_id: RunId) -> Option<Arc<RunReceipt>> {
        let runs = self.runs.read().await;
        let entry = runs.get(&run_id)?;
        match &*entry.sender.borrow() {
            RunObservation::Terminal(receipt) => Some(receipt.clone()),
            RunObservation::Active(_) => None,
        }
    }

    pub async fn update_agent(&self, mut snapshot: AgentSnapshot) {
        if let Some(entry) = self.agents.write().await.get_mut(&snapshot.agent_id) {
            snapshot.mailbox_depth = entry.snapshot.mailbox_depth;
            entry.snapshot = snapshot.clone();
        }
        self.persist_agent(&snapshot);
    }

    pub async fn agent_command_sender(
        &self,
        agent_id: AgentId,
    ) -> Option<mpsc::UnboundedSender<AgentCommand>> {
        self.agents
            .read()
            .await
            .get(&agent_id)
            .and_then(|entry| entry.commands.clone())
    }

    pub async fn agent_command_senders(&self) -> Vec<mpsc::UnboundedSender<AgentCommand>> {
        self.agents
            .read()
            .await
            .values()
            .filter_map(|entry| entry.commands.clone())
            .collect()
    }

    pub async fn set_mailbox_depth(&self, agent_id: AgentId, depth: usize) {
        let snapshot = {
            let mut agents = self.agents.write().await;
            agents.get_mut(&agent_id).map(|entry| {
                entry.snapshot.mailbox_depth = depth;
                entry.snapshot.clone()
            })
        };
        if let Some(snapshot) = snapshot {
            self.persist_agent(&snapshot);
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
                RunObservation::Active(snapshot) => snapshot.as_ref().clone(),
                RunObservation::Terminal(receipt) => RunSnapshot {
                    run_id: receipt.run_id,
                    agent_id: receipt.agent_id,
                    parent_run_id: receipt.parent_run_id,
                    session_stamp: receipt.session_stamp.clone(),
                    provider_identity: receipt.provider_identity.as_ref().map(|summary| {
                        crate::ProviderExecutionIdentity {
                            provider: summary.provider,
                            driver_id: summary.driver_id.clone(),
                            driver_revision: summary.driver_revision,
                            target: summary.target.clone(),
                            executable_path: summary.executable_path.clone(),
                            launch_sha256: summary.launch_sha256.clone(),
                            observed_version: summary.observed_version.clone(),
                            observed_components: summary.observed_components.clone(),
                            acp_protocol_version: summary.acp_protocol_version,
                            acp_agent_info: summary.acp_agent_info.clone(),
                            capability_digest: summary.capability_digest.clone(),
                            assertion_result: summary.assertion_result.clone(),
                        }
                    }),
                    state: receipt.state.into(),
                    stage: crate::RunStage::Terminal,
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

    pub async fn agent_exists(&self, agent_id: AgentId) -> bool {
        self.agents.read().await.contains_key(&agent_id)
    }

    fn persist_agent(&self, snapshot: &AgentSnapshot) {
        if let Some(store) = &self.store
            && let Err(error) = store.save_agent(snapshot)
        {
            tracing::error!(%error, "failed to persist Agent metadata");
        }
    }

    fn persist_run_snapshot(&self, snapshot: &RunSnapshot) {
        if let Some(store) = &self.store
            && let Err(error) = store.save_run_snapshot(snapshot)
        {
            tracing::error!(%error, "failed to persist Run metadata");
        }
    }

    fn persist_receipt(&self, receipt: &RunReceipt) {
        if let Some(store) = &self.store
            && let Err(error) = store.save_receipt(receipt)
        {
            tracing::error!(%error, "failed to persist terminal receipt");
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(None)
    }
}
