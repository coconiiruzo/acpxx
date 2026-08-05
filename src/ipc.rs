//! Versioned, length-delimited local IPC over a user-owned Unix Domain Socket.

use std::future::Future;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use uuid::Uuid;

use crate::{
    AdmissionError, AgentHandle, AgentMessage, Broker, ControlError, FollowupTask,
    InterruptReceipt, ListQuery, ListSnapshot, MessageReceipt, NonEmpty, RunEvent, RunHandle,
    RunId, RunReceipt, SpawnReceipt, SpawnRequest, WaitOptions,
};

pub const IPC_VERSION: u16 = 2;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope {
    pub version: u16,
    pub request_id: Uuid,
    pub command: IpcCommand,
}

impl RequestEnvelope {
    #[must_use]
    pub fn new(command: IpcCommand) -> Self {
        Self {
            version: IPC_VERSION,
            request_id: Uuid::now_v7(),
            command,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation", content = "arguments")]
pub enum IpcCommand {
    Spawn(SpawnRequest),
    Send {
        agent: AgentHandle,
        message: AgentMessage,
    },
    Followup {
        agent: AgentHandle,
        after: RunId,
        task: FollowupTask,
    },
    Interrupt(RunHandle),
    List(ListQuery),
    WaitRun {
        run: RunHandle,
        timeout_ms: Option<u64>,
    },
    WaitAny {
        runs: Vec<RunHandle>,
        timeout_ms: Option<u64>,
    },
    WaitAll {
        runs: Vec<RunHandle>,
        timeout_ms: Option<u64>,
    },
    Watch(RunHandle),
    CompatibilityStatus,
    ReloadCompatibility,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum IpcResponse {
    Spawn(SpawnReceipt),
    Message(MessageReceipt),
    RunHandle(RunHandle),
    Interrupt(InterruptReceipt),
    List(ListSnapshot),
    Run(RunReceipt),
    Runs(Vec<RunReceipt>),
    Event(RunEvent),
    StreamEnd(RunReceipt),
    CompatibilityStatus(crate::CatalogStatus),
    Error(IpcError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

impl IpcError {
    fn protocol(message: impl Into<String>) -> Self {
        Self {
            code: "ipc_protocol_error".into(),
            message: message.into(),
        }
    }
}

impl From<ControlError> for IpcError {
    fn from(error: ControlError) -> Self {
        let code = match &error {
            ControlError::Admission(admission) => match admission {
                AdmissionError::InvalidProvider { .. } => "invalid_provider",
                AdmissionError::InvalidCwd { .. } => "invalid_cwd",
                AdmissionError::AgentNotFound(_) => "agent_not_found",
                AdmissionError::RunNotFound(_) => "run_not_found",
                AdmissionError::HandleMismatch { .. } => "handle_mismatch",
                AdmissionError::StaleParent(_) => "stale_parent",
                AdmissionError::AgentBusy(_) => "agent_busy",
                AdmissionError::ContinuityAlreadyLost(_) => "continuity_already_lost",
                AdmissionError::InvalidRequest(_) => "invalid_request",
            },
            ControlError::InvalidTransition(_) => "invalid_transition",
            ControlError::NotImplemented { .. } => "not_implemented",
            ControlError::WaitTimeout { .. } => "wait_timeout",
            ControlError::ActorClosed => "actor_closed",
            ControlError::Internal(_) => "internal",
        };
        Self {
            code: code.into(),
            message: error.to_string(),
        }
    }
}

pub struct IpcClient {
    socket: PathBuf,
}

impl IpcClient {
    #[must_use]
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    pub async fn request(&self, command: IpcCommand) -> io::Result<IpcResponse> {
        let mut stream = UnixStream::connect(&self.socket).await?;
        write_json_frame(&mut stream, &RequestEnvelope::new(command)).await?;
        read_json_frame(&mut stream).await
    }

    pub async fn watch(
        &self,
        run: RunHandle,
    ) -> io::Result<tokio::sync::mpsc::Receiver<IpcResponse>> {
        let mut stream = UnixStream::connect(&self.socket).await?;
        write_json_frame(&mut stream, &RequestEnvelope::new(IpcCommand::Watch(run))).await?;
        let (sender, receiver) = tokio::sync::mpsc::channel(128);
        tokio::spawn(async move {
            while let Ok(response) = read_json_frame(&mut stream).await {
                let terminal =
                    matches!(response, IpcResponse::StreamEnd(_) | IpcResponse::Error(_));
                if sender.send(response).await.is_err() || terminal {
                    break;
                }
            }
        });
        Ok(receiver)
    }
}

pub struct LocalServer {
    socket: PathBuf,
    socket_identity: (u64, u64),
    listener: UnixListener,
}

impl LocalServer {
    pub fn bind(socket: impl Into<PathBuf>) -> io::Result<Self> {
        let socket = socket.into();
        if let Some(parent) = socket.parent() {
            ensure_private_directory(parent)?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        prepare_socket_path(&socket)?;
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        let metadata = std::fs::symlink_metadata(&socket)?;
        Ok(Self {
            socket,
            socket_identity: (metadata.dev(), metadata.ino()),
            listener,
        })
    }

    pub async fn serve_until(
        self,
        broker: Broker,
        shutdown: impl Future<Output = ()>,
    ) -> io::Result<()> {
        tokio::pin!(shutdown);
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (stream, _) = accepted?;
                    let broker = broker.clone();
                    connections.spawn(async move {
                        let _ = handle_connection(stream, broker).await;
                    });
                }
                () = &mut shutdown => break,
            }
        }
        connections.abort_all();
        while connections.join_next().await.is_some() {}
        broker
            .shutdown()
            .await
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.cleanup_socket()
    }

    fn cleanup_socket(&self) -> io::Result<()> {
        let metadata = match std::fs::symlink_metadata(&self.socket) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if (metadata.dev(), metadata.ino()) != self.socket_identity {
            return Ok(());
        }
        std::fs::remove_file(&self.socket)
    }
}

fn prepare_socket_path(path: &Path) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "IPC path exists and is not a current-user socket",
        ));
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "an agentmux broker is already listening",
        )),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            let current = std::fs::symlink_metadata(path)?;
            if (current.dev(), current.ino()) == (metadata.dev(), metadata.ino()) {
                std::fs::remove_file(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "IPC socket changed while checking stale state",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.cleanup_socket();
    }
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "IPC directory must be a real directory, not a symlink",
                ));
            }
            if metadata.uid() != unsafe { libc::geteuid() } {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "IPC directory is not owned by the current user",
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::fs::create_dir_all(path),
        Err(error) => Err(error),
    }
}

#[must_use]
pub fn default_socket_path() -> PathBuf {
    // macOS limits `sockaddr_un.sun_path` to 104 bytes; `$TMPDIR` is commonly
    // already long enough to make a nested socket unusable.
    PathBuf::from("/tmp")
        .join(format!("agentmux-{}", unsafe { libc::geteuid() }))
        .join("broker.sock")
}

async fn handle_connection(mut stream: UnixStream, broker: Broker) -> io::Result<()> {
    let request: RequestEnvelope = read_json_frame(&mut stream).await?;
    if request.version != IPC_VERSION {
        return write_json_frame(
            &mut stream,
            &IpcResponse::Error(IpcError::protocol(format!(
                "IPC version {} is unsupported; expected {IPC_VERSION}",
                request.version
            ))),
        )
        .await;
    }
    if let IpcCommand::Watch(run) = request.command {
        return serve_watch(&mut stream, &broker, run).await;
    }
    let response = dispatch(&broker, request.command).await;
    write_json_frame(&mut stream, &response).await
}

async fn dispatch(broker: &Broker, command: IpcCommand) -> IpcResponse {
    let result: crate::Result<IpcResponse> = match command {
        IpcCommand::Spawn(request) => broker.spawn(request).await.map(IpcResponse::Spawn),
        IpcCommand::Send { agent, message } => {
            broker.send(agent, message).await.map(IpcResponse::Message)
        }
        IpcCommand::Followup { agent, after, task } => broker
            .followup(agent, after, task)
            .await
            .map(IpcResponse::RunHandle),
        IpcCommand::Interrupt(run) => broker.interrupt(run).await.map(IpcResponse::Interrupt),
        IpcCommand::List(query) => broker.list(query).await.map(IpcResponse::List),
        IpcCommand::WaitRun { run, timeout_ms } => broker
            .wait_run(run, wait_options(timeout_ms))
            .await
            .map(IpcResponse::Run),
        IpcCommand::WaitAny { runs, timeout_ms } => match into_non_empty(runs) {
            Ok(runs) => broker
                .wait_any(runs, wait_options(timeout_ms))
                .await
                .map(IpcResponse::Run),
            Err(error) => Err(error),
        },
        IpcCommand::WaitAll { runs, timeout_ms } => match into_non_empty(runs) {
            Ok(runs) => broker
                .wait_all(runs, wait_options(timeout_ms))
                .await
                .map(IpcResponse::Runs),
            Err(error) => Err(error),
        },
        IpcCommand::CompatibilityStatus => Ok(IpcResponse::CompatibilityStatus(
            broker.compatibility_status(),
        )),
        IpcCommand::ReloadCompatibility => broker
            .reload_compatibility()
            .map(IpcResponse::CompatibilityStatus),
        IpcCommand::Watch(_) => unreachable!("watch is handled before dispatch"),
    };
    result.unwrap_or_else(|error| IpcResponse::Error(error.into()))
}

async fn serve_watch(stream: &mut UnixStream, broker: &Broker, run: RunHandle) -> io::Result<()> {
    let mut events = match broker.events(run) {
        Ok(events) => events,
        Err(error) => return write_json_frame(stream, &IpcResponse::Error(error.into())).await,
    };
    let terminal = broker.wait_run(run, WaitOptions::default());
    tokio::pin!(terminal);
    loop {
        tokio::select! {
            biased;
            event = events.next() => match event {
                Some(event) => write_json_frame(stream, &IpcResponse::Event(event)).await?,
                None => return Ok(()),
            },
            receipt = &mut terminal => {
                let response = match receipt {
                    Ok(receipt) => IpcResponse::StreamEnd(receipt),
                    Err(error) => IpcResponse::Error(error.into()),
                };
                return write_json_frame(stream, &response).await;
            }
        }
    }
}

fn wait_options(timeout_ms: Option<u64>) -> WaitOptions {
    WaitOptions {
        timeout: timeout_ms.map(Duration::from_millis),
    }
}

fn into_non_empty(runs: Vec<RunHandle>) -> crate::Result<NonEmpty<RunHandle>> {
    let mut runs = runs.into_iter();
    let head = runs.next().ok_or_else(|| {
        ControlError::from(AdmissionError::InvalidRequest(
            "aggregate wait requires at least one RunHandle".into(),
        ))
    })?;
    Ok(NonEmpty {
        head,
        tail: runs.collect(),
    })
}

async fn read_json_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut (impl AsyncRead + Unpin),
) -> io::Result<T> {
    let length = reader.read_u32().await? as usize;
    if length > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC frame exceeds the 1 MiB limit",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

async fn write_json_frame<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC frame exceeds the 1 MiB limit",
        ));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

#[must_use]
pub fn socket_is_user_only(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o777 == 0o600)
        .unwrap_or(false)
}
