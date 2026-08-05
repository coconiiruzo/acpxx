use std::ffi::OsString;
use std::io;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncWriteExt, copy};
use tokio::process::Command;

const TERMINATE_GRACE: Duration = Duration::from_secs(2);
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_millis(250);

pub async fn supervise(
    command: Vec<OsString>,
    allowed_env: &[String],
) -> std::io::Result<std::process::ExitStatus> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    for name in allowed_env {
        if let Some(value) = std::env::var_os(name) {
            process.env(name, value);
        }
    }
    #[cfg(unix)]
    {
        process.process_group(0);
    }
    let mut child = process.spawn()?;
    let process_group = child.id().ok_or_else(|| {
        std::io::Error::other("provider process has no PID after successful spawn")
    })?;
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("provider stdin was not piped"))?;
    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("provider stdout was not piped"))?;

    let mut input = tokio::spawn(async move {
        let result = copy_watchdog(&mut child_stdin).await;
        let _ = child_stdin.shutdown().await;
        result
    });
    let mut output = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let result = copy(&mut child_stdout, &mut stdout).await;
        let _ = stdout.shutdown().await;
        result
    });

    let mut wait = Box::pin(child.wait());
    let status = tokio::select! {
        status = &mut wait => status?,
        _ = &mut input => {
            terminate_process_group(process_group, libc::SIGTERM);
            match tokio::time::timeout(TERMINATE_GRACE, &mut wait).await {
                Ok(status) => status?,
                Err(_) => {
                    terminate_process_group(process_group, libc::SIGKILL);
                    wait.await?
                }
            }
        }
    };
    // The foreground process may exit while descendants still hold stdio or continue running.
    // The group was created by this supervisor, so this is the final cleanup barrier on every
    // exit path, including normal provider completion.
    terminate_process_group(process_group, libc::SIGKILL);
    input.abort();
    let _ = input.await;
    if tokio::time::timeout(OUTPUT_DRAIN_GRACE, &mut output)
        .await
        .is_err()
    {
        output.abort();
        let _ = output.await;
    }
    Ok(status)
}

#[cfg(unix)]
async fn copy_watchdog(writer: &mut (impl tokio::io::AsyncWrite + Unpin)) -> io::Result<u64> {
    use std::io::Read;
    use std::os::fd::FromRawFd;

    let descriptor = unsafe { libc::dup(libc::STDIN_FILENO) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        unsafe {
            libc::close(descriptor);
        }
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
    let input = tokio::io::unix::AsyncFd::new(file)?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let mut ready = input.readable().await?;
        match ready.try_io(|inner| {
            let mut file = inner.get_ref();
            file.read(&mut buffer)
        }) {
            Ok(Ok(0)) => return Ok(copied),
            Ok(Ok(read)) => {
                writer.write_all(&buffer[..read]).await?;
                copied = copied.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
            }
            Ok(Err(error)) => return Err(error),
            Err(_would_block) => continue,
        }
    }
}

#[cfg(not(unix))]
async fn copy_watchdog(writer: &mut (impl tokio::io::AsyncWrite + Unpin)) -> io::Result<u64> {
    tokio::io::copy(&mut tokio::io::stdin(), writer).await
}

#[cfg(unix)]
fn terminate_process_group(process_group: u32, signal: libc::c_int) {
    let group = i32::try_from(process_group).unwrap_or(i32::MAX);
    // SAFETY: the negative PID targets only the process group created above. Errors are benign
    // races with normal provider exit and are deliberately ignored.
    unsafe {
        libc::kill(-group, signal);
    }
}

#[cfg(not(unix))]
fn terminate_process_group(_process_group: u32, _signal: libc::c_int) {}
