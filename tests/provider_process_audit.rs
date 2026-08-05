#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use uuid::Uuid;

#[test]
#[ignore = "starts the authenticated Grok profile and kills a real broker"]
fn grok_real_process_tree_is_reaped_after_broker_death() {
    audit_profile("grok-default");
}

#[test]
#[ignore = "starts the authenticated Cursor profile and kills a real broker"]
fn cursor_real_process_tree_is_reaped_after_broker_death() {
    audit_profile("cursor-default");
}

#[test]
#[ignore = "starts the authenticated Codex profile and kills a real broker"]
fn codex_real_process_tree_is_reaped_after_broker_death() {
    audit_profile("codex-default");
}

#[test]
#[ignore = "starts the authenticated Claude profile and kills a real broker"]
fn claude_real_process_tree_is_reaped_after_broker_death() {
    audit_profile("claude-default");
}

fn audit_profile(profile: &str) {
    let fixture = AuditFixture::new(profile);
    let mut broker = fixture.start_broker();
    fixture.wait_for_socket();
    fixture.spawn_agent();
    let descendants = wait_for_descendants(broker.id(), 2, Duration::from_secs(10));
    assert!(
        descendants.len() >= 2,
        "{profile} did not create the expected supervisor/provider tree: {descendants:?}"
    );

    broker.kill_and_wait();
    for pid in descendants {
        assert_pid_dies(pid, Duration::from_secs(5));
    }
}

struct AuditFixture {
    profile: String,
    directory: PathBuf,
    socket: PathBuf,
    database: PathBuf,
}

impl AuditFixture {
    fn new(profile: &str) -> Self {
        let id = Uuid::now_v7().as_simple().to_string();
        let directory = PathBuf::from("/tmp").join(format!("amx-audit-{}", &id[id.len() - 12..]));
        std::fs::create_dir(&directory).unwrap();
        Self {
            profile: profile.into(),
            socket: directory.join("broker.sock"),
            database: directory.join("metadata.sqlite3"),
            directory,
        }
    }

    fn start_broker(&self) -> BrokerChild {
        BrokerChild(Some(
            Command::new(env!("CARGO_BIN_EXE_agentmux"))
                .arg("--socket")
                .arg(&self.socket)
                .arg("serve")
                .arg("--database")
                .arg(&self.database)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        ))
    }

    fn wait_for_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.socket.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("broker did not create {}", self.socket.display());
    }

    fn spawn_agent(&self) {
        let output = Command::new(env!("CARGO_BIN_EXE_agentmux"))
            .arg("--socket")
            .arg(&self.socket)
            .arg("spawn")
            .args(["--profile", &self.profile, "--cwd"])
            .arg(env!("CARGO_MANIFEST_DIR"))
            .arg("Remain active long enough for a process ownership audit")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "spawn failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

struct BrokerChild(Option<Child>);

impl BrokerChild {
    fn id(&self) -> u32 {
        self.0.as_ref().unwrap().id()
    }

    fn kill_and_wait(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            child.wait().unwrap();
        }
    }
}

impl Drop for BrokerChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for AuditFixture {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.database.as_os_str().to_owned();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn wait_for_descendants(root: u32, minimum: usize, timeout: Duration) -> BTreeSet<u32> {
    let deadline = Instant::now() + timeout;
    let mut observed = BTreeSet::new();
    while Instant::now() < deadline {
        observed.extend(descendants_of(root));
        if observed.len() >= minimum {
            return observed;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    observed
}

fn descendants_of(root: u32) -> BTreeSet<u32> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .unwrap();
    let relationships = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect::<Vec<(u32, u32)>>();
    let mut descendants = BTreeSet::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for &(pid, ppid) in &relationships {
            if ppid == parent && descendants.insert(pid) {
                frontier.push(pid);
            }
        }
    }
    descendants
}

fn assert_pid_dies(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process_alive(pid),
        "owned process {pid} survived broker death"
    );
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
