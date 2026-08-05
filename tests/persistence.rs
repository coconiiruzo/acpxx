mod support;

use std::path::{Path, PathBuf};

use acpxx::{
    AdmissionError, AgentSnapshot, Broker, Continuity, ContinuityLossReason, ControlError,
    FailureCode, FollowupTask, ListQuery, ProviderId, RunFailure, RunSnapshot, RunStage,
    StopReason, TerminalRunState, WaitOptions,
};
use support::mock_request;
use uuid::Uuid;

#[tokio::test]
async fn terminal_receipt_survives_restart_without_transcript_or_session_secret() {
    let database = database_path();
    let mut request = mock_request("normal", 0.0);
    request.task.content = "private-prompt-marker-never-persist".into();
    let broker = Broker::with_sqlite(1, &database).await.unwrap();
    let spawned = broker.spawn(request).await.unwrap();
    let live = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(live.output.text, "mock-ok");
    let session_id = live
        .session_stamp
        .as_ref()
        .unwrap()
        .provider_session_id
        .clone();
    broker.shutdown().await.unwrap();
    drop(broker);

    let persisted = persisted_bytes(&database);
    let persisted = String::from_utf8_lossy(&persisted);
    assert!(!persisted.contains("private-prompt-marker-never-persist"));
    assert!(!persisted.contains("mock-ok"));
    assert!(!persisted.contains(&session_id));

    let restored = Broker::with_sqlite(1, &database).await.unwrap();
    let receipt = restored
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Succeeded);
    assert!(receipt.output.text.is_empty());
    assert!(receipt.session_stamp.is_none());
    let snapshot = restored
        .list(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(
        snapshot.agents[0].continuity,
        Some(Continuity::Lost(ContinuityLossReason::HostRestarted))
    );
    assert!(matches!(
        restored
            .followup(
                spawned.agent,
                spawned.run.run_id,
                FollowupTask::new("must not resume")
            )
            .await,
        Err(ControlError::Admission(
            AdmissionError::ContinuityAlreadyLost(_)
        ))
    ));
    assert!(!restored.interrupt(spawned.run).await.unwrap().requested);
    cleanup_database(&database);
}

#[test]
fn nonterminal_run_is_reconciled_to_host_restarted() {
    let database = database_path();
    let store = acpxx::storage::MetadataStore::open(&database).unwrap();
    let agent_id = acpxx::AgentId::new();
    let run_id = acpxx::RunId::new();
    store
        .save_agent(&AgentSnapshot {
            agent_id,
            provider: ProviderId::Grok,
            provider_identity: None,
            process_alive: true,
            continuity: None,
            provider_capabilities: None,
            broker_capabilities: acpxx::BrokerCapabilitySnapshot::default(),
            active_run_id: Some(run_id),
            latest_run_id: Some(run_id),
            mailbox_depth: 0,
            display_name: None,
            display_path: None,
            cwd: PathBuf::from("/tmp"),
        })
        .unwrap();
    store
        .save_run_snapshot(&RunSnapshot::queued(
            run_id,
            agent_id,
            std::time::SystemTime::now(),
        ))
        .unwrap();
    let restored = store.restore_and_reconcile().unwrap();
    assert_eq!(restored.terminal_runs.len(), 1);
    let receipt = &restored.terminal_runs[0];
    assert_eq!(receipt.state, TerminalRunState::Failed);
    assert_eq!(
        receipt.failure.as_ref().map(|failure| failure.code),
        Some(FailureCode::HostRestarted)
    );
    assert_eq!(
        restored.agents[0].continuity,
        Some(Continuity::Lost(ContinuityLossReason::HostRestarted))
    );
    cleanup_database(&database);
}

#[test]
fn schema_v0_is_migrated_transactionally_to_v3() {
    let database = database_path();
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_metadata (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO schema_metadata(key, value) VALUES('schema_version', 0);
             CREATE TABLE agents (
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
        .unwrap();
    drop(connection);

    acpxx::storage::MetadataStore::open(&database).unwrap();
    let connection = rusqlite::Connection::open(&database).unwrap();
    let version: i64 = connection
        .query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 3);
    let has_completion_sequence = connection
        .prepare("PRAGMA table_info(runs)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == "completion_sequence");
    assert!(has_completion_sequence);
    assert!(has_column(&connection, "agents", "provider_identity_json"));
    assert!(has_column(&connection, "runs", "provider_identity_json"));
    assert!(!has_column(&connection, "agents", "provider_lock_json"));
    assert!(!has_column(&connection, "runs", "provider_lock_json"));
    drop(connection);
    cleanup_database(&database);
}

#[test]
fn schema_v1_is_migrated_to_v3_without_inventing_identity() {
    let database = database_path();
    let agent_id = acpxx::AgentId::new();
    let run_id = acpxx::RunId::new();
    let agent = historical_agent(agent_id, run_id);
    let run = RunSnapshot::queued(run_id, agent_id, std::time::SystemTime::now());
    let connection = rusqlite::Connection::open(&database).unwrap();
    create_legacy_schema(&connection, 1, false);
    connection
        .execute(
            "INSERT INTO agents(agent_id, snapshot_json) VALUES(?1, ?2)",
            rusqlite::params![agent_id.to_string(), serde_json::to_string(&agent).unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO runs(run_id, agent_id, snapshot_json) VALUES(?1, ?2, ?3)",
            rusqlite::params![
                run_id.to_string(),
                agent_id.to_string(),
                serde_json::to_string(&run).unwrap()
            ],
        )
        .unwrap();
    drop(connection);

    let store = acpxx::storage::MetadataStore::open(&database).unwrap();
    let restored = store.restore_and_reconcile().unwrap();
    assert!(restored.agents[0].provider_identity.is_none());
    assert!(restored.terminal_runs[0].provider_identity.is_none());
    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(schema_version(&connection), 3);
    assert!(has_column(&connection, "agents", "provider_identity_json"));
    assert!(has_column(&connection, "runs", "provider_identity_json"));
    drop(connection);
    cleanup_database(&database);
}

#[test]
fn catalog_schema_v2_is_migrated_to_observed_identity_without_authorization_fields() {
    let database = database_path();
    let agent_id = acpxx::AgentId::new();
    let run_id = acpxx::RunId::new();
    let mut agent_json = serde_json::to_value(historical_agent(agent_id, run_id)).unwrap();
    replace_identity_with_legacy_lock(&mut agent_json, legacy_full_lock());

    let mut snapshot = RunSnapshot::queued(run_id, agent_id, std::time::SystemTime::now());
    snapshot.start(std::time::SystemTime::now()).unwrap();
    snapshot
        .finish(
            TerminalRunState::Succeeded,
            StopReason::EndTurn,
            None,
            std::time::SystemTime::now(),
        )
        .unwrap();
    let mut snapshot_json = serde_json::to_value(snapshot).unwrap();
    replace_identity_with_legacy_lock(&mut snapshot_json, legacy_full_lock());

    let receipt = acpxx::RunReceipt {
        run_id,
        agent_id,
        parent_run_id: None,
        session_stamp: None,
        provider: ProviderId::Grok,
        provider_identity: None,
        state: TerminalRunState::Succeeded,
        queued_at: std::time::SystemTime::now(),
        started_at: std::time::SystemTime::now(),
        finished_at: std::time::SystemTime::now(),
        completion_sequence: 41,
        stop_reason: StopReason::EndTurn,
        failure: None,
        session_epoch: 1,
        output: Default::default(),
        metrics: Default::default(),
        cleanup: acpxx::CleanupReceipt {
            complete: true,
            process: acpxx::ProcessDisposition::Terminated,
        },
    };
    let mut receipt_json = serde_json::to_value(receipt).unwrap();
    replace_identity_with_legacy_lock(&mut receipt_json, legacy_lock_summary());

    let connection = rusqlite::Connection::open(&database).unwrap();
    create_legacy_schema(&connection, 2, true);
    connection
        .execute(
            "INSERT INTO agents(agent_id, snapshot_json, provider_lock_json) VALUES(?1, ?2, ?3)",
            rusqlite::params![
                agent_id.to_string(),
                serde_json::to_string(&agent_json).unwrap(),
                serde_json::to_string(&legacy_full_lock()).unwrap()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO runs(run_id, agent_id, snapshot_json, receipt_json, completion_sequence, provider_lock_json)
             VALUES(?1, ?2, ?3, ?4, 41, ?5)",
            rusqlite::params![
                run_id.to_string(),
                agent_id.to_string(),
                serde_json::to_string(&snapshot_json).unwrap(),
                serde_json::to_string(&receipt_json).unwrap(),
                serde_json::to_string(&legacy_full_lock()).unwrap()
            ],
        )
        .unwrap();
    drop(connection);

    let store = acpxx::storage::MetadataStore::open(&database).unwrap();
    let restored = store.restore_and_reconcile().unwrap();
    let identity = restored.agents[0]
        .provider_identity
        .as_ref()
        .expect("agent identity must be migrated");
    assert_eq!(
        identity.launch_sha256,
        "abababababababababababababababababababababababababababababababab"
    );
    assert_eq!(
        identity.observed_version.observed().map(String::as_str),
        Some("future-build")
    );
    assert_eq!(
        identity
            .observed_components
            .get("engine")
            .and_then(acpxx::ProbeObservation::observed)
            .map(String::as_str),
        Some("engine-next")
    );
    assert!(identity.acp_protocol_version.is_none());
    assert!(identity.acp_agent_info.is_none());
    assert!(identity.capability_digest.is_none());
    let receipt = &restored.terminal_runs[0];
    assert_eq!(receipt.completion_sequence, 41);
    assert_eq!(
        receipt
            .provider_identity
            .as_ref()
            .and_then(|identity| identity.observed_version.observed())
            .map(String::as_str),
        Some("future-build")
    );

    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(schema_version(&connection), 3);
    assert!(!has_column(&connection, "agents", "provider_lock_json"));
    assert!(!has_column(&connection, "runs", "provider_lock_json"));
    let migrated_json: String = connection
        .query_row("SELECT snapshot_json FROM agents", [], |row| row.get(0))
        .unwrap();
    for removed in [
        "provider_lock",
        "compatibility",
        "catalog_entry_id",
        "catalog_sequence",
        "catalog_digest",
        "qualification",
    ] {
        assert!(
            !migrated_json.contains(removed),
            "found removed field {removed}"
        );
    }
    assert!(migrated_json.contains("provider_identity"));
    drop(connection);
    cleanup_database(&database);
}

#[test]
fn schema_migration_failure_rolls_back_original_database() {
    let database = database_path();
    let connection = rusqlite::Connection::open(&database).unwrap();
    create_legacy_schema(&connection, 2, true);
    connection
        .execute(
            "INSERT INTO agents(agent_id, snapshot_json) VALUES('agent', 'not-json')",
            [],
        )
        .unwrap();
    drop(connection);

    assert!(acpxx::storage::MetadataStore::open(&database).is_err());
    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(schema_version(&connection), 2);
    assert!(has_column(&connection, "agents", "provider_lock_json"));
    assert!(!table_exists(&connection, "agents_pre_v3"));
    assert_eq!(
        connection
            .query_row("SELECT snapshot_json FROM agents", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "not-json"
    );
    drop(connection);
    cleanup_database(&database);
}

#[test]
fn unknown_future_schema_is_rejected_without_rewriting_its_version() {
    let database = database_path();
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_metadata (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO schema_metadata(key, value) VALUES('schema_version', 99);",
        )
        .unwrap();
    drop(connection);

    assert!(acpxx::storage::MetadataStore::open(&database).is_err());
    let connection = rusqlite::Connection::open(&database).unwrap();
    let version: i64 = connection
        .query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 99);
    drop(connection);
    cleanup_database(&database);
}

#[test]
fn corrupted_database_is_rejected() {
    let database = database_path();
    std::fs::write(&database, b"not a sqlite database").unwrap();
    assert!(acpxx::storage::MetadataStore::open(&database).is_err());
    cleanup_database(&database);
}

#[test]
fn provider_failure_credentials_are_redacted_before_sqlite_persistence() {
    let database = database_path();
    let store = acpxx::storage::MetadataStore::open(&database).unwrap();
    let mut snapshot = RunSnapshot::queued(
        acpxx::RunId::new(),
        acpxx::AgentId::new(),
        std::time::SystemTime::now(),
    );
    snapshot.start(std::time::SystemTime::now()).unwrap();
    snapshot
        .finish(
            TerminalRunState::Failed,
            StopReason::Failed,
            Some(RunFailure {
                code: FailureCode::PromptFailed,
                stage: RunStage::Prompting,
                retryable: false,
                message: "provider echoed OPENAI_API_KEY=sk-never-persist".into(),
            }),
            std::time::SystemTime::now(),
        )
        .unwrap();
    store.save_run_snapshot(&snapshot).unwrap();
    drop(store);

    let persisted = String::from_utf8_lossy(&persisted_bytes(&database)).into_owned();
    assert!(!persisted.contains("sk-never-persist"));
    assert!(persisted.contains("[REDACTED]"));
    cleanup_database(&database);
}

#[tokio::test]
async fn sqlite_write_failure_does_not_panic_or_stop_the_live_runtime() {
    let database = database_path();
    let broker = Broker::with_sqlite(1, &database).await.unwrap();
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER simulate_full_agents
               BEFORE INSERT ON agents
               BEGIN SELECT RAISE(FAIL, 'database or disk is full'); END;
             CREATE TRIGGER simulate_full_runs
               BEFORE INSERT ON runs
               BEGIN SELECT RAISE(FAIL, 'database or disk is full'); END;",
        )
        .unwrap();
    drop(connection);

    let spawned = broker.spawn(mock_request("normal", 0.0)).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(receipt.state, TerminalRunState::Succeeded);
    let snapshot = broker.list(Default::default()).await.unwrap();
    assert_eq!(snapshot.agents.len(), 1);
    assert_eq!(snapshot.runs.len(), 1);
    broker.shutdown().await.unwrap();
    cleanup_database(&database);
}

fn database_path() -> PathBuf {
    PathBuf::from("/tmp").join(format!("agentmux-metadata-{}.sqlite3", Uuid::now_v7()))
}

fn persisted_bytes(database: &Path) -> Vec<u8> {
    let mut bytes = std::fs::read(database).unwrap_or_default();
    for suffix in ["-wal", "-shm"] {
        let mut path = database.as_os_str().to_owned();
        path.push(suffix);
        bytes.extend(std::fs::read(PathBuf::from(path)).unwrap_or_default());
    }
    bytes
}

fn cleanup_database(database: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut path = database.as_os_str().to_owned();
        path.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
}

fn historical_agent(agent_id: acpxx::AgentId, run_id: acpxx::RunId) -> AgentSnapshot {
    AgentSnapshot {
        agent_id,
        provider: ProviderId::Grok,
        provider_identity: None,
        process_alive: false,
        continuity: Some(Continuity::Lost(ContinuityLossReason::HostRestarted)),
        provider_capabilities: None,
        broker_capabilities: acpxx::BrokerCapabilitySnapshot::default(),
        active_run_id: None,
        latest_run_id: Some(run_id),
        mailbox_depth: 0,
        display_name: None,
        display_path: None,
        cwd: PathBuf::from("/tmp"),
    }
}

fn create_legacy_schema(connection: &rusqlite::Connection, version: i64, catalog_columns: bool) {
    let identity_columns = if catalog_columns {
        ", provider_lock_json TEXT"
    } else {
        ""
    };
    connection
        .execute_batch(&format!(
            "CREATE TABLE schema_metadata (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO schema_metadata(key, value) VALUES('schema_version', {version});
             CREATE TABLE agents (
               agent_id TEXT PRIMARY KEY,
               snapshot_json TEXT NOT NULL
               {identity_columns}
             );
             CREATE TABLE runs (
               run_id TEXT PRIMARY KEY,
               agent_id TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               receipt_json TEXT,
               completion_sequence INTEGER NOT NULL DEFAULT 0
               {identity_columns}
             );"
        ))
        .unwrap();
}

fn replace_identity_with_legacy_lock(value: &mut serde_json::Value, lock: serde_json::Value) {
    let object = value.as_object_mut().unwrap();
    object.remove("provider_identity");
    object.insert("provider_lock".into(), lock);
}

fn legacy_full_lock() -> serde_json::Value {
    serde_json::json!({
        "provider": "grok",
        "driver_id": "grok-native-acp",
        "driver_revision": 7,
        "target": "aarch64-apple-darwin",
        "compatibility": "verified",
        "identity": {
            "display_version": "future-build",
            "normalized_version": "future-build",
            "components": { "engine": "engine-next" }
        },
        "artifacts": [{
            "subject": "executable",
            "algorithm": "sha256",
            "digest": "abababababababababababababababababababababababababababababababab"
        }],
        "catalog_entry_id": "grok/future",
        "catalog_sequence": 99,
        "catalog_digest": "obsolete-authorization-state",
        "qualification": { "state": "verified" }
    })
}

fn legacy_lock_summary() -> serde_json::Value {
    serde_json::json!({
        "provider": "grok",
        "compatibility": "verified",
        "display_version": "future-build",
        "components": { "engine": "engine-next" },
        "artifact_digests": [{
            "subject": "executable",
            "algorithm": "sha256",
            "digest": "abababababababababababababababababababababababababababababababab"
        }],
        "driver_id": "grok-native-acp",
        "driver_revision": 7,
        "catalog_entry_id": "grok/future",
        "catalog_sequence": 99,
        "catalog_digest": "obsolete-authorization-state"
    })
}

fn schema_version(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row(
            "SELECT value FROM schema_metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn has_column(connection: &rusqlite::Connection, table: &str, column: &str) -> bool {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == column)
}

fn table_exists(connection: &rusqlite::Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .unwrap()
}
