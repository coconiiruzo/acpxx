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

const SCHEMA_VERSION: i64 = 3;

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
        let redacted = redacted_agent(snapshot.clone());
        let json = serde_json::to_string(&redacted).map_err(internal)?;
        let identity = redacted
            .provider_identity
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(internal)?;
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .execute(
                "INSERT INTO agents(agent_id, snapshot_json, provider_identity_json)
                 VALUES(?1, ?2, ?3)
                 ON CONFLICT(agent_id) DO UPDATE SET
                   snapshot_json=excluded.snapshot_json,
                   provider_identity_json=excluded.provider_identity_json",
                params![snapshot.agent_id.to_string(), json, identity],
            )
            .map_err(internal)?;
        Ok(())
    }

    pub fn save_run_snapshot(&self, snapshot: &RunSnapshot) -> crate::Result<()> {
        let redacted = redacted_snapshot(snapshot.clone());
        let json = serde_json::to_string(&redacted).map_err(internal)?;
        let identity = redacted
            .provider_identity
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(internal)?;
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .execute(
                "INSERT INTO runs(run_id, agent_id, snapshot_json, provider_identity_json)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(run_id) DO UPDATE SET
                   snapshot_json=excluded.snapshot_json,
                   provider_identity_json=excluded.provider_identity_json",
                params![
                    snapshot.run_id.to_string(),
                    snapshot.agent_id.to_string(),
                    json,
                    identity
                ],
            )
            .map_err(internal)?;
        Ok(())
    }

    pub fn save_receipt(&self, receipt: &RunReceipt) -> crate::Result<()> {
        let redacted = redacted_receipt(receipt.clone());
        let json = serde_json::to_string(&redacted).map_err(internal)?;
        let identity = redacted
            .provider_identity
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(internal)?;
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .execute(
                "UPDATE runs SET receipt_json=?2, completion_sequence=?3,
                   provider_identity_json=?4 WHERE run_id=?1",
                params![
                    receipt.run_id.to_string(),
                    json,
                    i64::try_from(receipt.completion_sequence).unwrap_or(i64::MAX),
                    identity
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
                provider_identity: snapshot
                    .provider_identity
                    .as_ref()
                    .map(crate::ProviderExecutionIdentity::summary),
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
            create_v3_tables(&transaction)?;
            transaction
                .execute(
                    "INSERT INTO schema_metadata(key, value) VALUES('schema_version', ?1)",
                    [SCHEMA_VERSION],
                )
                .map_err(internal)?;
            transaction.commit().map_err(internal)
        }
        Some(0..=2) => migrate_to_v3(connection),
        Some(SCHEMA_VERSION) => create_v3_tables(connection),
        Some(version) => Err(internal(format!(
            "unsupported schema version {version}; expected {SCHEMA_VERSION}"
        ))),
    }
}

fn migrate_to_v3(connection: &mut Connection) -> crate::Result<()> {
    let transaction = connection.transaction().map_err(internal)?;
    if !table_exists(&transaction, "agents")? {
        transaction
            .execute_batch(
                "CREATE TABLE agents (
                   agent_id TEXT PRIMARY KEY,
                   snapshot_json TEXT NOT NULL
                 );
                 CREATE TABLE runs (
                   run_id TEXT PRIMARY KEY,
                   agent_id TEXT NOT NULL,
                   snapshot_json TEXT NOT NULL,
                   receipt_json TEXT
                 );",
            )
            .map_err(internal)?;
    }
    transaction
        .execute_batch(
            "ALTER TABLE agents RENAME TO agents_pre_v3;
             ALTER TABLE runs RENAME TO runs_pre_v3;",
        )
        .map_err(internal)?;
    create_v3_tables(&transaction)?;
    let agents = {
        let mut statement = transaction
            .prepare("SELECT agent_id, snapshot_json FROM agents_pre_v3")
            .map_err(internal)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(internal)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?
    };
    for (agent_id, snapshot) in agents {
        let (snapshot, identity) = migrate_identity_json(&snapshot)?;
        transaction
            .execute(
                "INSERT INTO agents(agent_id, snapshot_json, provider_identity_json)
                 VALUES(?1, ?2, ?3)",
                params![agent_id, snapshot, identity],
            )
            .map_err(internal)?;
    }
    let has_completion = has_column(&transaction, "runs_pre_v3", "completion_sequence")?;
    let query = if has_completion {
        "SELECT run_id, agent_id, snapshot_json, receipt_json, completion_sequence FROM runs_pre_v3"
    } else {
        "SELECT run_id, agent_id, snapshot_json, receipt_json, 0 FROM runs_pre_v3"
    };
    let runs = {
        let mut statement = transaction.prepare(query).map_err(internal)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(internal)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(internal)?
    };
    for (run_id, agent_id, snapshot, receipt, completion) in runs {
        let (snapshot, snapshot_identity) = migrate_identity_json(&snapshot)?;
        let (receipt, receipt_identity) = receipt
            .map(|json| migrate_identity_json(&json))
            .transpose()?
            .map_or((None, None), |(json, identity)| (Some(json), identity));
        transaction
            .execute(
                "INSERT INTO runs(
                   run_id, agent_id, snapshot_json, receipt_json,
                   completion_sequence, provider_identity_json
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run_id,
                    agent_id,
                    snapshot,
                    receipt,
                    completion,
                    receipt_identity.or(snapshot_identity)
                ],
            )
            .map_err(internal)?;
    }
    transaction
        .execute_batch("DROP TABLE agents_pre_v3; DROP TABLE runs_pre_v3;")
        .map_err(internal)?;
    transaction
        .execute(
            "UPDATE schema_metadata SET value=?1 WHERE key='schema_version'",
            [SCHEMA_VERSION],
        )
        .map_err(internal)?;
    transaction.commit().map_err(internal)
}

fn create_v3_tables(connection: &Connection) -> crate::Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
               agent_id TEXT PRIMARY KEY,
               snapshot_json TEXT NOT NULL,
               provider_identity_json TEXT
             );
             CREATE TABLE IF NOT EXISTS runs (
               run_id TEXT PRIMARY KEY,
               agent_id TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               receipt_json TEXT,
               completion_sequence INTEGER NOT NULL DEFAULT 0,
               provider_identity_json TEXT
             );",
        )
        .map_err(internal)?;
    if !has_column(connection, "runs", "completion_sequence")?
        || !has_column(connection, "agents", "provider_identity_json")?
        || !has_column(connection, "runs", "provider_identity_json")?
        || has_column(connection, "agents", "provider_lock_json")?
        || has_column(connection, "runs", "provider_lock_json")?
    {
        return Err(internal("schema version 3 columns are invalid"));
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> crate::Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(internal)
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

fn migrate_identity_json(json: &str) -> crate::Result<(String, Option<String>)> {
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(internal)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| internal("persisted snapshot must be a JSON object"))?;
    if !object.contains_key("provider_identity")
        && let Some(lock) = object.remove("provider_lock")
    {
        object.insert("provider_identity".into(), legacy_lock_identity(lock));
    }
    let identity = object
        .get("provider_identity")
        .filter(|value| !value.is_null())
        .map(serde_json::to_string)
        .transpose()
        .map_err(internal)?;
    Ok((serde_json::to_string(&value).map_err(internal)?, identity))
}

fn legacy_lock_identity(lock: serde_json::Value) -> serde_json::Value {
    let nested = lock.get("identity");
    let version = nested
        .and_then(|value| value.get("display_version"))
        .or_else(|| lock.get("display_version"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let components = nested
        .and_then(|value| value.get("components"))
        .or_else(|| lock.get("components"))
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| {
            (
                name,
                serde_json::json!({ "state": "observed", "detail": value }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let artifacts = lock
        .get("artifacts")
        .or_else(|| lock.get("artifact_digests"))
        .and_then(serde_json::Value::as_array);
    let launch_sha256 = artifacts
        .and_then(|artifacts| {
            artifacts.iter().find(|artifact| {
                artifact.get("subject").and_then(serde_json::Value::as_str) == Some("executable")
            })
        })
        .and_then(|artifact| artifact.get("digest"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    serde_json::json!({
        "provider": lock.get("provider").cloned().unwrap_or(serde_json::Value::Null),
        "driver_id": lock.get("driver_id").cloned().unwrap_or_else(|| serde_json::json!("historical")),
        "driver_revision": lock.get("driver_revision").cloned().unwrap_or_else(|| serde_json::json!(0)),
        "target": lock.get("target").cloned().unwrap_or_else(|| serde_json::json!("unknown")),
        "executable_path": "",
        "launch_sha256": launch_sha256,
        "observed_version": { "state": "observed", "detail": version },
        "observed_components": components,
        "acp_protocol_version": null,
        "acp_agent_info": null,
        "capability_digest": null,
        "assertion_result": { "status": "not_configured" }
    })
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
