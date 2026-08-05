#![cfg(unix)]

mod support;

use std::time::Duration;

use acpxx::ipc::{
    IpcClient, IpcCommand, IpcResponse, LocalServer, RequestEnvelope, socket_is_user_only,
};
use acpxx::{AgentMessage, Broker, FollowupTask, ListQuery, RunHandle, TerminalRunState};
use support::mock_request;
use uuid::Uuid;

#[tokio::test]
async fn separate_clients_share_one_persistent_agent() {
    let socket = socket_path();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = LocalServer::bind(&socket).unwrap();
    assert!(socket_is_user_only(&socket));
    assert!(
        LocalServer::bind(&socket).is_err(),
        "second broker must be rejected"
    );
    let task = tokio::spawn(server.serve_until(Broker::new(2), async {
        let _ = stopped.await;
    }));

    let first_client = IpcClient::new(&socket);
    let spawned = match first_client
        .request(IpcCommand::Spawn(mock_request("ipc", 0.05)))
        .await
        .unwrap()
    {
        IpcResponse::Spawn(receipt) => receipt,
        response => panic!("unexpected spawn response: {response:?}"),
    };
    drop(first_client);

    let second_client = IpcClient::new(&socket);
    let first = wait(&second_client, spawned.run).await;
    assert_eq!(first.state, TerminalRunState::Succeeded);
    let first_stamp = first.session_stamp.clone();

    let message = second_client
        .request(IpcCommand::Send {
            agent: spawned.agent,
            message: AgentMessage {
                content: "queued over another CLI connection".into(),
            },
        })
        .await
        .unwrap();
    assert!(matches!(message, IpcResponse::Message(_)));
    let followup = match second_client
        .request(IpcCommand::Followup {
            agent: spawned.agent,
            after: spawned.run.run_id,
            task: FollowupTask::new("continue"),
        })
        .await
        .unwrap()
    {
        IpcResponse::RunHandle(handle) => handle,
        response => panic!("unexpected followup response: {response:?}"),
    };
    let second = wait(&second_client, followup).await;
    assert_eq!(second.session_stamp, first_stamp);

    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(!socket.exists());
}

#[tokio::test]
async fn client_disconnect_does_not_interrupt_a_run() {
    let socket = socket_path();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = LocalServer::bind(&socket).unwrap();
    let task = tokio::spawn(server.serve_until(Broker::new(1), async {
        let _ = stopped.await;
    }));

    let spawned = {
        let client = IpcClient::new(&socket);
        match client
            .request(IpcCommand::Spawn(mock_request("disconnect", 0.15)))
            .await
            .unwrap()
        {
            IpcResponse::Spawn(receipt) => receipt,
            response => panic!("unexpected spawn response: {response:?}"),
        }
    };
    tokio::time::sleep(Duration::from_millis(25)).await;
    let client = IpcClient::new(&socket);
    assert_eq!(
        wait(&client, spawned.run).await.state,
        TerminalRunState::Succeeded
    );

    let snapshot = match client
        .request(IpcCommand::List(ListQuery {
            agent_id: Some(spawned.agent.agent_id),
            ..ListQuery::default()
        }))
        .await
        .unwrap()
    {
        IpcResponse::List(snapshot) => snapshot,
        response => panic!("unexpected list response: {response:?}"),
    };
    assert!(snapshot.agents[0].process_alive);

    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn watch_streams_events_and_a_terminal_receipt() {
    let socket = socket_path();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = LocalServer::bind(&socket).unwrap();
    let task = tokio::spawn(server.serve_until(Broker::new(1), async {
        let _ = stopped.await;
    }));
    let client = IpcClient::new(&socket);
    let spawned = match client
        .request(IpcCommand::Spawn(mock_request("watch", 0.5)))
        .await
        .unwrap()
    {
        IpcResponse::Spawn(receipt) => receipt,
        response => panic!("unexpected spawn response: {response:?}"),
    };
    let mut stream = client.watch(spawned.run).await.unwrap();
    let mut saw_event = false;
    loop {
        match stream.recv().await.unwrap() {
            IpcResponse::Event(event) => {
                saw_event = true;
                assert_eq!(event.run_id, spawned.run.run_id);
            }
            IpcResponse::StreamEnd(receipt) => {
                assert_eq!(receipt.state, TerminalRunState::Succeeded);
                break;
            }
            response => panic!("unexpected watch response: {response:?}"),
        }
    }
    assert!(saw_event);
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn protocol_version_mismatch_is_explicit() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let socket = socket_path();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = LocalServer::bind(&socket).unwrap();
    let task = tokio::spawn(server.serve_until(Broker::new(1), async {
        let _ = stopped.await;
    }));
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let request = RequestEnvelope {
        version: 999,
        request_id: Uuid::now_v7(),
        command: IpcCommand::List(ListQuery::default()),
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    let length = stream.read_u32().await.unwrap();
    let mut bytes = vec![0; length as usize];
    stream.read_exact(&mut bytes).await.unwrap();
    let response: IpcResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(matches!(
        response,
        IpcResponse::Error(ref error) if error.code == "ipc_protocol_error"
    ));
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn catalog_status_and_reload_are_explicit_ipc_v2_operations() {
    let socket = socket_path();
    let database = std::env::temp_dir().join(format!("agentmux-ipc-{}.sqlite3", Uuid::now_v7()));
    let catalog_root = std::env::temp_dir().join(format!("agentmux-catalog-{}", Uuid::now_v7()));
    let broker = Broker::with_sqlite_options_and_catalog(
        1,
        &database,
        &catalog_root,
        Duration::from_secs(30),
        std::iter::empty(),
    )
    .await
    .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = LocalServer::bind(&socket).unwrap();
    let task = tokio::spawn(server.serve_until(broker, async {
        let _ = stopped.await;
    }));
    let client = IpcClient::new(&socket);
    for command in [
        IpcCommand::CompatibilityStatus,
        IpcCommand::ReloadCompatibility,
    ] {
        let response = client.request(command).await.unwrap();
        assert!(matches!(
            response,
            IpcResponse::CompatibilityStatus(ref status)
                if status.catalog_id == "agentmux-official" && status.sequence == 1
        ));
    }
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", database.display()));
    }
    let _ = std::fs::remove_dir_all(catalog_root);
}

#[tokio::test]
async fn oversized_frame_is_rejected_without_stopping_the_broker() {
    use acpxx::ipc::MAX_FRAME_SIZE;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let socket = socket_path();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = LocalServer::bind(&socket).unwrap();
    let task = tokio::spawn(server.serve_until(Broker::new(1), async {
        let _ = stopped.await;
    }));

    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    stream.write_u32((MAX_FRAME_SIZE + 1) as u32).await.unwrap();
    let mut byte = [0_u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut byte))
        .await
        .expect("server must reject the frame promptly");
    assert!(matches!(read, Ok(0) | Err(_)));

    let client = IpcClient::new(&socket);
    assert!(matches!(
        client
            .request(IpcCommand::List(ListQuery::default()))
            .await
            .unwrap(),
        IpcResponse::List(_)
    ));
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn stale_owned_socket_is_recovered_but_active_socket_is_not_replaced() {
    let socket = socket_path();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let stale = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    drop(stale);
    assert!(socket.exists());

    let server = LocalServer::bind(&socket).unwrap();
    assert!(LocalServer::bind(&socket).is_err());
    drop(server);
    assert!(!socket.exists());
}

async fn wait(client: &IpcClient, run: RunHandle) -> acpxx::RunReceipt {
    match client
        .request(IpcCommand::WaitRun {
            run,
            timeout_ms: Some(2_000),
        })
        .await
        .unwrap()
    {
        IpcResponse::Run(receipt) => receipt,
        response => panic!("unexpected wait response: {response:?}"),
    }
}

fn socket_path() -> std::path::PathBuf {
    let id = Uuid::now_v7().as_simple().to_string();
    std::path::PathBuf::from("/tmp")
        .join(format!("amx-{}", &id[id.len() - 12..]))
        .join("broker.sock")
}
