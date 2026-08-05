#![cfg(unix)]

mod support;

use std::process::{Command, Stdio};
use std::time::Duration;

use acpxx::{Broker, ProcessDisposition, Task, WaitOptions};
use support::mock_request;
use uuid::Uuid;

#[tokio::test]
async fn broker_shutdown_terminates_retained_provider_grandchildren() {
    let pid_file = std::env::temp_dir().join(format!("acpxx-grandchild-{}.pid", Uuid::now_v7()));
    let mut request = mock_request("grandchild", 0.0);
    request.task = Task::new(format!(
        "return the fixture output __fake_mode=grandchild __fake_pid_file={}",
        pid_file.display()
    ));

    let broker = Broker::new(1);
    let spawned = broker.spawn(request).await.unwrap();
    let receipt = broker
        .wait_run(spawned.run, WaitOptions::default())
        .await
        .unwrap();
    assert!(receipt.cleanup.complete);
    assert_eq!(receipt.cleanup.process, ProcessDisposition::Retained);

    broker.shutdown().await.unwrap();

    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("mock must publish grandchild PID")
        .parse()
        .unwrap();
    let mut alive = true;
    for _ in 0..20 {
        alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if alive {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stderr(Stdio::null())
            .status();
    }
    let _ = std::fs::remove_file(&pid_file);
    assert!(!alive, "grandchild process {pid} survived broker shutdown");
}
