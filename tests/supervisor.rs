#![cfg(unix)]

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use uuid::Uuid;

const PROCESS_EXIT_WAIT: Duration = Duration::from_secs(2);

#[test]
fn watchdog_eof_terminates_provider_process_group() {
    let pid_file = std::env::temp_dir().join(format!("agentmux-watchdog-{}.pid", Uuid::now_v7()));
    let script = r#"
import subprocess, sys, time
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
with open(sys.argv[1], "w", encoding="utf-8") as output:
    output.write(str(child.pid))
time.sleep(60)
"#;
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .arg("__supervise")
        .args(["--allow-env", "PATH", "--"])
        .arg("python3")
        .arg("-c")
        .arg(script)
        .arg(&pid_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    for _ in 0..100 {
        if pid_file.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("provider must publish grandchild PID")
        .parse()
        .unwrap();

    supervisor.stdin.take().unwrap().flush().unwrap();
    let status = supervisor.wait().unwrap();
    assert!(
        !status.success(),
        "TERM-terminated provider should be non-zero"
    );

    let alive = wait_until_dead(pid, PROCESS_EXIT_WAIT);
    if alive {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stderr(Stdio::null())
            .status();
    }
    let _ = std::fs::remove_file(pid_file);
    assert!(!alive, "supervisor left grandchild {pid} alive");
}

#[test]
fn normal_provider_exit_also_cleans_lingering_descendants() {
    let pid_file =
        std::env::temp_dir().join(format!("agentmux-normal-exit-{}.pid", Uuid::now_v7()));
    let script = r#"
import subprocess, sys
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
with open(sys.argv[1], "w", encoding="utf-8") as output:
    output.write(str(child.pid))
"#;
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .arg("__supervise")
        .args(["--allow-env", "PATH", "--"])
        .arg("python3")
        .arg("-c")
        .arg(script)
        .arg(&pid_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _watchdog = supervisor.stdin.take().unwrap();
    let status = supervisor.wait().unwrap();
    assert!(status.success());
    let pid = read_pid(&pid_file);
    let alive = wait_until_dead(pid, PROCESS_EXIT_WAIT);
    if alive {
        force_kill(pid);
    }
    let _ = std::fs::remove_file(pid_file);
    assert!(!alive, "normal provider exit left descendant {pid} alive");
}

#[test]
fn supervisor_drains_provider_stdout_before_returning() {
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .arg("__supervise")
        .arg("--")
        .arg("/bin/sh")
        .arg("-c")
        .arg("printf final-frame")
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let _watchdog = supervisor.stdin.take().unwrap();
    let mut stdout = supervisor.stdout.take().unwrap();
    let status = supervisor.wait().unwrap();
    let mut output = Vec::new();
    stdout.read_to_end(&mut output).unwrap();
    assert!(status.success());
    assert_eq!(output, b"final-frame");
}

#[test]
fn repeated_short_lived_providers_are_reaped() {
    short_lived_provider_soak(16);
}

#[test]
#[ignore = "runs the full Phase 17 1,000-process lifecycle soak"]
fn one_thousand_short_lived_provider_processes_are_reaped() {
    short_lived_provider_soak(1_000);
}

fn short_lived_provider_soak(iterations: usize) {
    let descriptors_before = open_descriptor_count();
    for iteration in 0..iterations {
        let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agentmux"))
            .arg("__supervise")
            .arg("--")
            .arg("/usr/bin/true")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let _watchdog = supervisor.stdin.take().unwrap();
        let status = supervisor.wait().unwrap();
        assert!(status.success(), "iteration {iteration} failed");
    }
    if let (Some(before), Some(after)) = (descriptors_before, open_descriptor_count()) {
        assert!(
            after <= before + 2,
            "file descriptor count grew from {before} to {after}"
        );
    }
}

#[test]
fn detached_descendant_fixture_is_classified_as_unsupported() {
    #[derive(Debug, Eq, PartialEq)]
    enum ConformanceResult {
        Contained,
        UnsupportedProcessEscape,
    }

    let pid_file = std::env::temp_dir().join(format!("agentmux-escape-{}.pid", Uuid::now_v7()));
    let script = r#"
import subprocess, sys, time
child = subprocess.Popen(
    [sys.executable, "-c", "import time; time.sleep(60)"],
    start_new_session=True,
)
with open(sys.argv[1], "w", encoding="utf-8") as output:
    output.write(str(child.pid))
time.sleep(60)
"#;
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .arg("__supervise")
        .args(["--allow-env", "PATH", "--"])
        .arg("python3")
        .arg("-c")
        .arg(script)
        .arg(&pid_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_pid_file(&pid_file);
    let pid = read_pid(&pid_file);
    drop(supervisor.stdin.take());
    let _ = supervisor.wait().unwrap();

    let result = if process_alive(pid) {
        ConformanceResult::UnsupportedProcessEscape
    } else {
        ConformanceResult::Contained
    };
    if process_alive(pid) {
        force_kill(pid);
    }
    let _ = std::fs::remove_file(pid_file);
    assert_eq!(result, ConformanceResult::UnsupportedProcessEscape);
}

#[test]
fn supervisor_passes_only_allowlisted_environment_variables() {
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .arg("__supervise")
        .args(["--allow-env", "AGENTMUX_VISIBLE", "--"])
        .arg("/bin/sh")
        .arg("-c")
        .arg("printf '%s|%s' \"$AGENTMUX_VISIBLE\" \"$AGENTMUX_SECRET\"")
        .env("AGENTMUX_VISIBLE", "allowed")
        .env("AGENTMUX_SECRET", "must-not-leak")
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let _watchdog = supervisor.stdin.take().unwrap();
    let output = supervisor.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"allowed|");
}

fn wait_for_pid_file(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("fixture did not publish {}", path.display());
}

fn read_pid(path: &std::path::Path) -> u32 {
    std::fs::read_to_string(path)
        .expect("fixture must publish descendant PID")
        .parse()
        .unwrap()
}

fn wait_until_dead(pid: u32, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !process_alive(pid) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    process_alive(pid)
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn force_kill(pid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .stderr(Stdio::null())
        .status();
}

fn open_descriptor_count() -> Option<usize> {
    std::fs::read_dir("/dev/fd").ok().map(Iterator::count)
}
