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
fn schema_v0_is_migrated_transactionally_to_v1() {
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
    assert_eq!(version, 1);
    let has_completion_sequence = connection
        .prepare("PRAGMA table_info(runs)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.unwrap() == "completion_sequence");
    assert!(has_completion_sequence);
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
