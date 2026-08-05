use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, SessionId, TerminalExitStatus, TerminalId,
    TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse,
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{Mutex, watch};
use tokio::task::AbortHandle;
use uuid::Uuid;

const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;
const MAX_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
const SAFE_INHERITED_ENV: &[&str] = &["PATH", "TMPDIR", "LANG", "LC_ALL", "SHELL"];

#[derive(Clone, Debug)]
pub struct TerminalHost {
    inner: Arc<TerminalHostInner>,
}

#[derive(Debug)]
struct TerminalHostInner {
    root: PathBuf,
    allow_execution: bool,
    terminals: Mutex<HashMap<String, Arc<TerminalEntry>>>,
}

impl Drop for TerminalHostInner {
    fn drop(&mut self) {
        // This is the cancellation-safety backstop for an aborted ACP session task. Normal
        // shutdown uses the async grace path, but dropping the last host handle must still ensure
        // that owned terminal process groups cannot outlive the broker.
        if let Ok(terminals) = self.terminals.try_lock() {
            for entry in terminals.values() {
                signal_process_group(entry.process_group, libc::SIGKILL);
                abort_readers(entry);
            }
        }
    }
}

#[derive(Debug)]
struct TerminalEntry {
    session_id: String,
    process_group: u32,
    output: Arc<StdMutex<BoundedOutput>>,
    exit: watch::Receiver<ExitState>,
    readers: Vec<AbortHandle>,
    kill_lock: Mutex<()>,
}

#[derive(Clone, Debug)]
enum ExitState {
    Running,
    Exited(TerminalExitStatus),
    WaitFailed(String),
}

#[derive(Debug)]
struct BoundedOutput {
    text: String,
    limit: usize,
    truncated: bool,
}

impl BoundedOutput {
    fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        self.text.push_str(&text);
        if self.text.len() <= self.limit {
            return;
        }
        self.truncated = true;
        let mut start = self.text.len() - self.limit;
        while start < self.text.len() && !self.text.is_char_boundary(start) {
            start += 1;
        }
        self.text.drain(..start);
    }

    fn snapshot(&self) -> (String, bool) {
        (self.text.clone(), self.truncated)
    }
}

impl TerminalHost {
    pub fn new(root: PathBuf, allow_execution: bool) -> std::io::Result<Self> {
        Ok(Self {
            inner: Arc::new(TerminalHostInner {
                root: std::fs::canonicalize(root)?,
                allow_execution,
                terminals: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub async fn create(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, agent_client_protocol::Error> {
        if !self.inner.allow_execution {
            return Err(protocol_error("terminal execution is denied by policy"));
        }
        if request.command.is_empty() {
            return Err(protocol_error("terminal command must not be empty"));
        }
        let cwd = request.cwd.unwrap_or_else(|| self.inner.root.clone());
        if !cwd.is_absolute() {
            return Err(protocol_error("terminal cwd must be absolute"));
        }
        let cwd = std::fs::canonicalize(cwd).map_err(protocol_io_error)?;
        if !cwd.starts_with(&self.inner.root) {
            return Err(protocol_error("terminal cwd escapes the Agent root"));
        }
        for variable in &request.env {
            if variable.name.is_empty()
                || variable.name.contains('=')
                || variable.name.contains('\0')
                || variable.value.contains('\0')
            {
                return Err(protocol_error(
                    "terminal environment contains an invalid entry",
                ));
            }
        }

        let limit = request
            .output_byte_limit
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(DEFAULT_OUTPUT_LIMIT)
            .min(MAX_OUTPUT_LIMIT);
        let mut command = Command::new(request.command);
        command
            .args(request.args)
            .current_dir(cwd)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for name in SAFE_INHERITED_ENV {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for variable in request.env {
            command.env(variable.name, variable.value);
        }
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command.spawn().map_err(protocol_io_error)?;
        let process_group = child
            .id()
            .ok_or_else(|| protocol_error("terminal process has no PID after spawn"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| protocol_error("terminal stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| protocol_error("terminal stderr was not piped"))?;
        let output = Arc::new(StdMutex::new(BoundedOutput::new(limit)));
        let stdout_reader = tokio::spawn(capture_output(stdout, output.clone()));
        let stderr_reader = tokio::spawn(capture_output(stderr, output.clone()));
        let reader_abort_handles = vec![stdout_reader.abort_handle(), stderr_reader.abort_handle()];
        let (exit_tx, exit_rx) = watch::channel(ExitState::Running);
        tokio::spawn(async move {
            let child_status = child.wait().await;
            let _ = stdout_reader.await;
            let _ = stderr_reader.await;
            let state = match child_status {
                Ok(status) => ExitState::Exited(exit_status(status)),
                Err(error) => ExitState::WaitFailed(error.to_string()),
            };
            exit_tx.send_replace(state);
        });

        let terminal_id = TerminalId::new(format!("term_{}", Uuid::now_v7()));
        self.inner.terminals.lock().await.insert(
            terminal_id.to_string(),
            Arc::new(TerminalEntry {
                session_id: request.session_id.to_string(),
                process_group,
                output,
                exit: exit_rx,
                readers: reader_abort_handles,
                kill_lock: Mutex::new(()),
            }),
        );
        Ok(CreateTerminalResponse::new(terminal_id))
    }

    pub async fn output(
        &self,
        request: TerminalOutputRequest,
    ) -> Result<TerminalOutputResponse, agent_client_protocol::Error> {
        let entry = self
            .entry(&request.session_id, &request.terminal_id)
            .await?;
        let (output, truncated) = entry
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot();
        let exit_status = match &*entry.exit.borrow() {
            ExitState::Running => None,
            ExitState::Exited(status) => Some(status.clone()),
            ExitState::WaitFailed(message) => return Err(protocol_error(message)),
        };
        Ok(TerminalOutputResponse::new(output, truncated).exit_status(exit_status))
    }

    pub async fn wait_for_exit(
        &self,
        request: WaitForTerminalExitRequest,
    ) -> Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
        let entry = self
            .entry(&request.session_id, &request.terminal_id)
            .await?;
        Ok(WaitForTerminalExitResponse::new(
            wait_for_exit_state(entry.exit.clone()).await?,
        ))
    }

    pub async fn kill(
        &self,
        request: KillTerminalRequest,
    ) -> Result<KillTerminalResponse, agent_client_protocol::Error> {
        let entry = self
            .entry(&request.session_id, &request.terminal_id)
            .await?;
        terminate_entry(&entry).await?;
        Ok(KillTerminalResponse::new())
    }

    pub async fn release(
        &self,
        request: ReleaseTerminalRequest,
    ) -> Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
        let key = request.terminal_id.to_string();
        let entry = self
            .entry(&request.session_id, &request.terminal_id)
            .await?;
        self.inner.terminals.lock().await.remove(&key);
        terminate_entry(&entry).await?;
        abort_readers(&entry);
        Ok(ReleaseTerminalResponse::new())
    }

    pub async fn kill_all(&self) {
        let entries = self
            .inner
            .terminals
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        futures::future::join_all(entries.iter().map(terminate_entry)).await;
    }

    pub async fn shutdown(&self) {
        let entries = {
            let mut terminals = self.inner.terminals.lock().await;
            terminals
                .drain()
                .map(|(_, entry)| entry)
                .collect::<Vec<_>>()
        };
        futures::future::join_all(entries.iter().map(terminate_entry)).await;
        for entry in &entries {
            abort_readers(entry);
        }
    }

    async fn entry(
        &self,
        session_id: &SessionId,
        terminal_id: &TerminalId,
    ) -> Result<Arc<TerminalEntry>, agent_client_protocol::Error> {
        let entry = self
            .inner
            .terminals
            .lock()
            .await
            .get(&terminal_id.to_string())
            .cloned()
            .ok_or_else(|| protocol_error("unknown terminal ID"))?;
        if entry.session_id != session_id.to_string() {
            return Err(protocol_error("terminal belongs to a different session"));
        }
        Ok(entry)
    }
}

async fn capture_output<R>(mut reader: R, output: Arc<StdMutex<BoundedOutput>>)
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => output
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(&buffer[..read]),
        }
    }
}

async fn terminate_entry(entry: &Arc<TerminalEntry>) -> Result<(), agent_client_protocol::Error> {
    let _guard = entry.kill_lock.lock().await;
    signal_process_group(entry.process_group, libc::SIGTERM);
    let mut exit = entry.exit.clone();
    if !matches!(&*exit.borrow(), ExitState::Running) {
        signal_process_group(entry.process_group, libc::SIGKILL);
        return exit_result(&exit.borrow());
    }
    if tokio::time::timeout(TERMINATE_GRACE, exit.changed())
        .await
        .is_err()
    {
        signal_process_group(entry.process_group, libc::SIGKILL);
        if tokio::time::timeout(TERMINATE_GRACE, exit.changed())
            .await
            .is_err()
        {
            abort_readers(entry);
            exit.changed()
                .await
                .map_err(|_| protocol_error("terminal exit observer closed"))?;
        }
    } else {
        // The foreground command may have exited while descendants remain in its group.
        signal_process_group(entry.process_group, libc::SIGKILL);
    }
    exit_result(&exit.borrow())
}

async fn wait_for_exit_state(
    mut exit: watch::Receiver<ExitState>,
) -> Result<TerminalExitStatus, agent_client_protocol::Error> {
    loop {
        match &*exit.borrow_and_update() {
            ExitState::Running => {}
            ExitState::Exited(status) => return Ok(status.clone()),
            ExitState::WaitFailed(message) => return Err(protocol_error(message)),
        }
        exit.changed()
            .await
            .map_err(|_| protocol_error("terminal exit observer closed"))?;
    }
}

fn exit_result(exit: &ExitState) -> Result<(), agent_client_protocol::Error> {
    match exit {
        ExitState::Exited(_) => Ok(()),
        ExitState::WaitFailed(message) => Err(protocol_error(message)),
        ExitState::Running => Err(protocol_error("terminal did not exit")),
    }
}

fn abort_readers(entry: &Arc<TerminalEntry>) {
    for reader in &entry.readers {
        reader.abort();
    }
}

#[cfg(unix)]
fn signal_process_group(process_group: u32, signal: libc::c_int) {
    let group = i32::try_from(process_group).unwrap_or(i32::MAX);
    // SAFETY: the negative PID targets only the dedicated process group created in `create`.
    unsafe {
        libc::kill(-group, signal);
    }
}

#[cfg(not(unix))]
fn signal_process_group(_process_group: u32, _signal: libc::c_int) {}

fn exit_status(status: std::process::ExitStatus) -> TerminalExitStatus {
    let mut result = TerminalExitStatus::new()
        .exit_code(status.code().and_then(|code| u32::try_from(code).ok()));
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        result = result.signal(status.signal().map(|signal| signal.to_string()));
    }
    result
}

fn protocol_io_error(error: std::io::Error) -> agent_client_protocol::Error {
    protocol_error(error.to_string())
}

fn protocol_error(message: impl ToString) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn default_policy_rejects_terminal_creation() {
        let root = std::env::current_dir().unwrap();
        let host = TerminalHost::new(root, false).unwrap();
        let request = CreateTerminalRequest::new("session", "/usr/bin/true");
        assert!(host.create(request).await.is_err());
    }

    #[tokio::test]
    async fn captures_bounded_output_and_exit_status() {
        let root = std::env::current_dir().unwrap();
        let host = TerminalHost::new(root.clone(), true).unwrap();
        let created = host
            .create(
                CreateTerminalRequest::new("session", "/bin/sh")
                    .args(vec!["-c".into(), "printf abcdef".into()])
                    .cwd(root)
                    .output_byte_limit(4),
            )
            .await
            .unwrap();
        let waited = host
            .wait_for_exit(WaitForTerminalExitRequest::new(
                "session",
                created.terminal_id.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(waited.exit_status.exit_code, Some(0));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let output = host
            .output(TerminalOutputRequest::new(
                "session",
                created.terminal_id.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(output.output, "cdef");
        assert!(output.truncated);
        host.release(ReleaseTerminalRequest::new("session", created.terminal_id))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_cwd_outside_root() {
        let root = std::env::current_dir().unwrap();
        let host = TerminalHost::new(root, true).unwrap();
        let request =
            CreateTerminalRequest::new("session", "/usr/bin/true").cwd(PathBuf::from("/tmp"));
        assert!(host.create(request).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_a_cwd_symlink_that_escapes_root() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("agentmux-term-cwd-{}", Uuid::now_v7()));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let host = TerminalHost::new(root.clone(), true).unwrap();
        let request =
            CreateTerminalRequest::new("session", "/usr/bin/true").cwd(root.join("escape"));
        assert!(host.create(request).await.is_err());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let mut output = BoundedOutput::new(5);
        output.push("あいう".as_bytes());
        assert!(output.text.is_char_boundary(0));
        assert!(output.text.len() <= 5);
        assert_eq!(output.text, "う");
        assert!(output.truncated);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn release_terminates_the_owned_process_group() {
        let root = std::env::current_dir().unwrap();
        let pid_file =
            std::env::temp_dir().join(format!("agentmux-terminal-{}.pid", Uuid::now_v7()));
        let host = TerminalHost::new(root.clone(), true).unwrap();
        let created = host
            .create(
                CreateTerminalRequest::new("session", "/bin/sh")
                    .args(vec![
                        "-c".into(),
                        "sleep 60 & echo $! > \"$1\"; wait".into(),
                        "agentmux-terminal-test".into(),
                        pid_file.to_string_lossy().into_owned(),
                    ])
                    .cwd(root),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !pid_file.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shell must publish its child PID");
        let pid = std::fs::read_to_string(&pid_file).unwrap();

        host.release(ReleaseTerminalRequest::new("session", created.terminal_id))
            .await
            .unwrap();

        let mut alive = true;
        for _ in 0..20 {
            alive = std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if alive {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", pid.trim()])
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_file(pid_file);
        assert!(!alive, "terminal descendant {pid} survived release");
    }
}
