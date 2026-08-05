//! Metadata-only SQLite persistence. Prompt, reasoning, tool arguments, provider
//! session identifiers, and assistant text are deliberately not persisted.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    AgentId, AgentSnapshot, CleanupReceipt, Continuity, ContinuityLossReason, FailureCode,
    OutputReceipt, ProcessDisposition, RunFailure, RunMetrics, RunReceipt, RunSnapshot, RunStage,
    StopReason, TerminalRunState,
};

pub const SCHEMA_VERSION: i64 = 2;

#[derive(Clone)]
pub struct MetadataStore {
    connection: Arc<Mutex<Connection>>,
}

pub struct RestoredMetadata {
    pub agents: Vec<AgentSnapshot>,
    pub terminal_runs: Vec<RunReceipt>,
}

impl MetadataStore {
    pub fn open(path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(internal)?;
        }
        let mut connection = Connection::open(path).map_err(internal)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA foreign_keys=ON;
                 CREATE TABLE IF NOT EXISTS schema_metadata (
                   key TEXT PRIMARY KEY,
                   value INTEGER NOT NULL
                 );",
            )
            .map_err(internal)?;
        initialize_schema(&mut connection)?;
        let integrity: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(internal)?;
        if integrity != "ok" {
            return Err(internal(format!("integrity check failed: {integrity}")));
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn save_agent(&self, snapshot: &AgentSnapshot) -> crate::Result<()> {
        let json = serde_json::to_string(&redacted_agent(snapshot.clone())).map_err(internal)?;
        let provider_lock = snapshot
            .provider_lock
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(internal)?;
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .execute(
                "INSERT INTO agents(agent_id, snapshot_json, provider_lock_json) VALUES(?1, ?2, ?3)
                 ON CONFLICT(agent_id) DO UPDATE SET
                   snapshot_json=excluded.snapshot_json,
                   provider_lock_json=excluded.provider_lock_json",
                params![snapshot.agent_id.to_string(), json, provider_lock],
            )
            .map_err(internal)?;
        Ok(())
    }

    pub fn save_run_snapshot(&self, snapshot: &RunSnapshot) -> crate::Result<()> {
        let json = serde_json::to_string(&redacted_snapshot(snapshot.clone())).map_err(internal)?;
        let provider_lock = snapshot
            .provider_lock
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(internal)?;
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .execute(
                "INSERT INTO runs(run_id, agent_id, snapshot_json, provider_lock_json)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(run_id) DO UPDATE SET
                   snapshot_json=excluded.snapshot_json,
                   provider_lock_json=excluded.provider_lock_json",
                params![
                    snapshot.run_id.to_string(),
                    snapshot.agent_id.to_string(),
                    json,
                    provider_lock
                ],
            )
            .map_err(internal)?;
        Ok(())
    }

    pub fn save_receipt(&self, receipt: &RunReceipt) -> crate::Result<()> {
        let json = serde_json::to_string(&redacted_receipt(receipt.clone())).map_err(internal)?;
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .execute(
                "UPDATE runs SET receipt_json=?2, completion_sequence=?3 WHERE run_id=?1",
                params![
                    receipt.run_id.to_string(),
                    json,
                    i64::try_from(receipt.completion_sequence).unwrap_or(i64::MAX)
                ],
            )
            .map_err(internal)?;
        Ok(())
    }

    pub fn restore_and_reconcile(&self) -> crate::Result<RestoredMetadata> {
        let mut agents = self.load_agents()?;
        let (active_runs, mut terminal_runs) = self.load_runs()?;
        let providers: HashMap<AgentId, crate::ProviderId> = agents
            .iter()
            .map(|agent| (agent.agent_id, agent.provider))
            .collect();
        let base_sequence = terminal_runs
            .iter()
            .map(|receipt| receipt.completion_sequence)
            .max()
            .unwrap_or(0);
        for (offset, snapshot) in active_runs.into_iter().enumerate() {
            let sequence = base_sequence
                .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
                .saturating_add(1);
            let receipt = RunReceipt {
                run_id: snapshot.run_id,
                agent_id: snapshot.agent_id,
                parent_run_id: snapshot.parent_run_id,
                session_stamp: None,
                provider: providers[&snapshot.agent_id],
                provider_lock: snapshot.provider_lock.as_ref().map(|lock| lock.summary()),
                state: TerminalRunState::Failed,
                queued_at: snapshot.queued_at,
                started_at: snapshot.started_at.unwrap_or(snapshot.queued_at),
                finished_at: SystemTime::now(),
                completion_sequence: sequence,
                stop_reason: StopReason::Failed,
                failure: Some(RunFailure {
                    code: FailureCode::HostRestarted,
                    stage: RunStage::Terminal,
                    retryable: false,
                    message: "broker restarted before the Run reached a terminal state".into(),
                }),
                session_epoch: 0,
                output: OutputReceipt::default(),
                metrics: RunMetrics::default(),
                cleanup: CleanupReceipt {
                    complete: true,
                    process: ProcessDisposition::Terminated,
                },
            };
            self.save_receipt(&receipt)?;
            terminal_runs.push(receipt);
        }
        for agent in &mut agents {
            agent.process_alive = false;
            agent.active_run_id = None;
            agent.mailbox_depth = 0;
            agent.provider_capabilities = None;
            agent.continuity = Some(Continuity::Lost(ContinuityLossReason::HostRestarted));
            self.save_agent(agent)?;
        }
        Ok(RestoredMetadata {
            agents,
            terminal_runs,
        })
    }

    fn load_agents(&self) -> crate::Result<Vec<AgentSnapshot>> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut statement = connection
            .prepare("SELECT snapshot_json FROM agents ORDER BY agent_id")
            .map_err(internal)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(internal)?;
        rows.map(|row| {
            let json = row.map_err(internal)?;
            serde_json::from_str(&json).map_err(internal)
        })
        .collect()
    }

    fn load_runs(&self) -> crate::Result<(Vec<RunSnapshot>, Vec<RunReceipt>)> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut statement = connection
            .prepare("SELECT snapshot_json, receipt_json FROM runs ORDER BY run_id")
            .map_err(internal)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(internal)?;
        let mut active = Vec::new();
        let mut terminal = Vec::new();
        for row in rows {
            let (snapshot, receipt) = row.map_err(internal)?;
            if let Some(receipt) = receipt {
                terminal.push(serde_json::from_str(&receipt).map_err(internal)?);
            } else {
                active.push(serde_json::from_str(&snapshot).map_err(internal)?);
            }
        }
        Ok((active, terminal))
    }
}

fn initialize_schema(connection: &mut Connection) -> crate::Result<()> {
    let version = connection
        .query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(internal)?;
    match version {
        None => {
            let transaction = connection.transaction().map_err(internal)?;
            create_v2_tables(&transaction)?;
            transaction
                .execute(
                    "INSERT INTO schema_metadata(key, value) VALUES('schema_version', ?1)",
                    [SCHEMA_VERSION],
                )
                .map_err(internal)?;
            transaction.commit().map_err(internal)
        }
        Some(0) => migrate_v0_to_v2(connection),
        Some(1) => migrate_v1_to_v2(connection),
        Some(SCHEMA_VERSION) => create_v2_tables(connection),
        Some(version) => Err(internal(format!(
            "unsupported schema version {version}; expected {SCHEMA_VERSION}"
        ))),
    }
}

fn migrate_v0_to_v2(connection: &mut Connection) -> crate::Result<()> {
    let transaction = connection.transaction().map_err(internal)?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
               agent_id TEXT PRIMARY KEY,
               snapshot_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
               run_id TEXT PRIMARY KEY,
               agent_id TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               receipt_json TEXT
             );",
        )
        .map_err(internal)?;
    if !has_column(&transaction, "runs", "completion_sequence")? {
        transaction
            .execute(
                "ALTER TABLE runs ADD COLUMN completion_sequence INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(internal)?;
    }
    add_v2_columns(&transaction)?;
    set_schema_version(&transaction, SCHEMA_VERSION)?;
    transaction.commit().map_err(internal)
}

fn migrate_v1_to_v2(connection: &mut Connection) -> crate::Result<()> {
    let transaction = connection.transaction().map_err(internal)?;
    create_v1_tables(&transaction)?;
    add_v2_columns(&transaction)?;
    set_schema_version(&transaction, SCHEMA_VERSION)?;
    transaction.commit().map_err(internal)
}

fn create_v2_tables(connection: &Connection) -> crate::Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
               agent_id TEXT PRIMARY KEY,
               snapshot_json TEXT NOT NULL,
               provider_lock_json TEXT
             );
             CREATE TABLE IF NOT EXISTS runs (
               run_id TEXT PRIMARY KEY,
               agent_id TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               receipt_json TEXT,
               completion_sequence INTEGER NOT NULL DEFAULT 0,
               provider_lock_json TEXT
             );",
        )
        .map_err(internal)?;
    for (table, column) in [
        ("runs", "completion_sequence"),
        ("runs", "provider_lock_json"),
        ("agents", "provider_lock_json"),
    ] {
        if !has_column(connection, table, column)? {
            return Err(internal(format!(
                "schema version 2 is missing {table}.{column}"
            )));
        }
    }
    Ok(())
}

fn create_v1_tables(connection: &Connection) -> crate::Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
               agent_id TEXT PRIMARY KEY,
               snapshot_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
               run_id TEXT PRIMARY KEY,
               agent_id TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               receipt_json TEXT,
               completion_sequence INTEGER NOT NULL DEFAULT 0
             );",
        )
        .map_err(internal)
}

fn add_v2_columns(connection: &Connection) -> crate::Result<()> {
    if !has_column(connection, "agents", "provider_lock_json")? {
        connection
            .execute("ALTER TABLE agents ADD COLUMN provider_lock_json TEXT", [])
            .map_err(internal)?;
    }
    if !has_column(connection, "runs", "provider_lock_json")? {
        connection
            .execute("ALTER TABLE runs ADD COLUMN provider_lock_json TEXT", [])
            .map_err(internal)?;
    }
    Ok(())
}

fn set_schema_version(connection: &Connection, version: i64) -> crate::Result<()> {
    connection
        .execute(
            "UPDATE schema_metadata SET value=?1 WHERE key='schema_version'",
            [version],
        )
        .map_err(internal)?;
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> crate::Result<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(internal)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(internal)?;
    for name in names {
        if name.map_err(internal)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn redacted_agent(mut snapshot: AgentSnapshot) -> AgentSnapshot {
    snapshot.continuity = snapshot.continuity.map(|continuity| match continuity {
        Continuity::Available(_) => Continuity::Lost(ContinuityLossReason::HostRestarted),
        lost => lost,
    });
    snapshot.provider_capabilities = None;
    snapshot
}

fn redacted_snapshot(mut snapshot: RunSnapshot) -> RunSnapshot {
    snapshot.session_stamp = None;
    if let Some(failure) = &mut snapshot.failure {
        failure.message = crate::security::redact_sensitive(&failure.message);
    }
    snapshot
}

fn redacted_receipt(mut receipt: RunReceipt) -> RunReceipt {
    receipt.session_stamp = None;
    receipt.output.text.clear();
    if let Some(failure) = &mut receipt.failure {
        failure.message = crate::security::redact_sensitive(&failure.message);
    }
    receipt
}

fn internal(error: impl std::fmt::Display) -> crate::ControlError {
    crate::ControlError::Internal(format!("SQLite metadata store: {error}"))
}
