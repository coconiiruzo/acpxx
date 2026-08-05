//! Synthetic Phase 17 performance qualification for the local broker.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

use crate::ipc::{IpcClient, IpcCommand, IpcResponse, LocalServer};
use crate::{Broker, ListQuery};

pub const COLD_STARTUP_P95_BUDGET_MS: f64 = 50.0;
pub const BROKER_IDLE_RSS_BUDGET_MIB: f64 = 30.0;
pub const IN_PROCESS_ADMISSION_P99_BUDGET_MS: f64 = 2.0;
pub const IPC_ADMISSION_P99_BUDGET_MS: f64 = 5.0;
pub const WAITER_WAKE_P99_BUDGET_MS: f64 = 5.0;
pub const EVENT_FAN_IN_BUDGET_PER_SECOND: f64 = 10_000.0;

#[derive(Clone, Copy, Debug)]
pub struct BenchmarkOptions {
    pub samples: usize,
    pub events: usize,
}

impl Default for BenchmarkOptions {
    fn default() -> Self {
        Self {
            samples: 100,
            events: 100_000,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkReport {
    pub agentmux_version: &'static str,
    pub host: BenchmarkHost,
    pub samples: usize,
    pub requested_events: usize,
    pub measurements: BenchmarkMeasurements,
    pub budgets: BenchmarkBudgets,
    pub qualification_eligible: bool,
    pub passed: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkHost {
    pub os: &'static str,
    pub architecture: &'static str,
    pub name: Option<String>,
    pub model: Option<String>,
    pub release_build: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkMeasurements {
    pub cold_startup_p95_ms: f64,
    pub broker_idle_rss_mib: f64,
    pub in_process_admission_p99_ms: f64,
    pub ipc_admission_p99_ms: f64,
    pub waiter_wakeup_p99_ms: f64,
    pub synthetic_event_fan_in_per_second: f64,
    pub synthetic_events_delivered: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkBudgets {
    pub cold_startup_p95_ms: f64,
    pub broker_idle_rss_mib: f64,
    pub in_process_admission_p99_ms: f64,
    pub ipc_admission_p99_ms: f64,
    pub waiter_wakeup_p99_ms: f64,
    pub synthetic_event_fan_in_per_second: f64,
}

impl Default for BenchmarkBudgets {
    fn default() -> Self {
        Self {
            cold_startup_p95_ms: COLD_STARTUP_P95_BUDGET_MS,
            broker_idle_rss_mib: BROKER_IDLE_RSS_BUDGET_MIB,
            in_process_admission_p99_ms: IN_PROCESS_ADMISSION_P99_BUDGET_MS,
            ipc_admission_p99_ms: IPC_ADMISSION_P99_BUDGET_MS,
            waiter_wakeup_p99_ms: WAITER_WAKE_P99_BUDGET_MS,
            synthetic_event_fan_in_per_second: EVENT_FAN_IN_BUDGET_PER_SECOND,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BenchmarkError {
    #[error("benchmark sample count must be between 10 and 10,000")]
    InvalidSamples,
    #[error("benchmark event count must be between 10,000 and 10,000,000")]
    InvalidEvents,
    #[error("benchmark I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("broker benchmark failed: {0}")]
    Broker(#[from] crate::ControlError),
    #[error("benchmark task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("synthetic event fan-in lost or duplicated an event")]
    EventIntegrity,
    #[error("benchmark IPC returned an unexpected response")]
    UnexpectedIpcResponse,
}

pub async fn run(options: BenchmarkOptions) -> Result<BenchmarkReport, BenchmarkError> {
    if !(10..=10_000).contains(&options.samples) {
        return Err(BenchmarkError::InvalidSamples);
    }
    if !(10_000..=10_000_000).contains(&options.events) {
        return Err(BenchmarkError::InvalidEvents);
    }

    let cold_startup_p95_ms = measure_cold_startup(options.samples).await?;
    let broker = Broker::new(1);
    let broker_idle_rss_mib = current_rss_mib().await?;
    let in_process_admission_p99_ms = measure_in_process(&broker, options.samples).await?;
    let ipc_admission_p99_ms = measure_ipc(broker, options.samples).await?;
    let waiter_wakeup_p99_ms = measure_waiter_wakeup(options.samples).await?;
    let (synthetic_event_fan_in_per_second, synthetic_events_delivered) =
        measure_event_fan_in(options.events).await?;

    let measurements = BenchmarkMeasurements {
        cold_startup_p95_ms,
        broker_idle_rss_mib,
        in_process_admission_p99_ms,
        ipc_admission_p99_ms,
        waiter_wakeup_p99_ms,
        synthetic_event_fan_in_per_second,
        synthetic_events_delivered,
    };
    let budgets = BenchmarkBudgets::default();
    let (host_name, host_model) = host_hardware().await;
    let qualification_eligible = cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && !cfg!(debug_assertions)
        && host_name.as_deref() == Some("Mac Studio");
    let passed =
        qualification_eligible.then(|| budgets_met(&measurements, &budgets, options.events));

    Ok(BenchmarkReport {
        agentmux_version: env!("CARGO_PKG_VERSION"),
        host: BenchmarkHost {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            name: host_name,
            model: host_model,
            release_build: !cfg!(debug_assertions),
        },
        samples: options.samples,
        requested_events: options.events,
        measurements,
        budgets,
        qualification_eligible,
        passed,
    })
}

fn budgets_met(
    measurements: &BenchmarkMeasurements,
    budgets: &BenchmarkBudgets,
    requested_events: usize,
) -> bool {
    measurements.cold_startup_p95_ms <= budgets.cold_startup_p95_ms
        && measurements.broker_idle_rss_mib <= budgets.broker_idle_rss_mib
        && measurements.in_process_admission_p99_ms <= budgets.in_process_admission_p99_ms
        && measurements.ipc_admission_p99_ms <= budgets.ipc_admission_p99_ms
        && measurements.waiter_wakeup_p99_ms <= budgets.waiter_wakeup_p99_ms
        && measurements.synthetic_event_fan_in_per_second
            >= budgets.synthetic_event_fan_in_per_second
        && measurements.synthetic_events_delivered == requested_events
}

async fn measure_cold_startup(samples: usize) -> Result<f64, BenchmarkError> {
    let executable = std::env::current_exe()?;
    let warmup = tokio::process::Command::new(&executable)
        .arg("--version")
        .output()
        .await?;
    if !warmup.status.success() {
        return Err(BenchmarkError::UnexpectedIpcResponse);
    }
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let output = tokio::process::Command::new(&executable)
            .arg("--version")
            .output()
            .await?;
        if !output.status.success() {
            return Err(BenchmarkError::UnexpectedIpcResponse);
        }
        timings.push(started.elapsed());
    }
    Ok(percentile_ms(&mut timings, 0.95))
}

async fn measure_in_process(broker: &Broker, samples: usize) -> Result<f64, BenchmarkError> {
    for _ in 0..10 {
        broker.list(ListQuery::default()).await?;
    }
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        broker.list(ListQuery::default()).await?;
        timings.push(started.elapsed());
    }
    Ok(percentile_ms(&mut timings, 0.99))
}

async fn measure_ipc(broker: Broker, samples: usize) -> Result<f64, BenchmarkError> {
    let root = benchmark_directory();
    let socket = root.join("broker.sock");
    let server = LocalServer::bind(&socket)?;
    let (shutdown, stop) = oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        server
            .serve_until(broker, async {
                let _ = stop.await;
            })
            .await
    });
    let client = IpcClient::new(&socket);
    for _ in 0..10 {
        require_list_response(
            client
                .request(IpcCommand::List(ListQuery::default()))
                .await?,
        )?;
    }
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        require_list_response(
            client
                .request(IpcCommand::List(ListQuery::default()))
                .await?,
        )?;
        timings.push(started.elapsed());
    }
    let _ = shutdown.send(());
    serving.await??;
    let _ = std::fs::remove_dir(&root);
    Ok(percentile_ms(&mut timings, 0.99))
}

fn require_list_response(response: IpcResponse) -> Result<(), BenchmarkError> {
    if matches!(response, IpcResponse::List(_)) {
        Ok(())
    } else {
        Err(BenchmarkError::UnexpectedIpcResponse)
    }
}

async fn measure_waiter_wakeup(samples: usize) -> Result<f64, BenchmarkError> {
    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let (sender, mut receiver) = watch::channel(false);
        let (ready, subscribed) = oneshot::channel();
        let waiter = tokio::spawn(async move {
            let _ = ready.send(());
            receiver
                .changed()
                .await
                .expect("watch sender remains alive");
            Instant::now()
        });
        let _ = subscribed.await;
        let started = Instant::now();
        sender.send_replace(true);
        timings.push(waiter.await?.saturating_duration_since(started));
    }
    Ok(percentile_ms(&mut timings, 0.99))
}

async fn measure_event_fan_in(events: usize) -> Result<(f64, usize), BenchmarkError> {
    let producer_count = 8.min(events);
    let (sender, mut receiver) = mpsc::channel::<usize>(1024);
    let consumer = tokio::spawn(async move {
        let mut seen = vec![false; events];
        let mut delivered = 0;
        while let Some(sequence) = receiver.recv().await {
            if sequence >= events || seen[sequence] {
                return Err(BenchmarkError::EventIntegrity);
            }
            seen[sequence] = true;
            delivered += 1;
        }
        if delivered != events || seen.iter().any(|value| !value) {
            return Err(BenchmarkError::EventIntegrity);
        }
        Ok(delivered)
    });
    let started = Instant::now();
    let mut producers = Vec::with_capacity(producer_count);
    for producer in 0..producer_count {
        let sender = sender.clone();
        producers.push(tokio::spawn(async move {
            let mut sequence = producer;
            while sequence < events {
                sender
                    .send(sequence)
                    .await
                    .map_err(|_| BenchmarkError::EventIntegrity)?;
                sequence += producer_count;
            }
            Ok::<_, BenchmarkError>(())
        }));
    }
    drop(sender);
    for producer in producers {
        producer.await??;
    }
    let delivered = consumer.await??;
    let elapsed = started.elapsed().as_secs_f64();
    Ok((delivered as f64 / elapsed.max(f64::EPSILON), delivered))
}

async fn current_rss_mib() -> Result<f64, BenchmarkError> {
    let output = tokio::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .await?;
    if !output.status.success() {
        return Err(BenchmarkError::UnexpectedIpcResponse);
    }
    let kib = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|_| BenchmarkError::UnexpectedIpcResponse)?;
    Ok(kib / 1024.0)
}

async fn host_hardware() -> (Option<String>, Option<String>) {
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("system_profiler")
            .args(["SPHardwareDataType", "-json"])
            .output()
            .await
            .ok();
        let Some(output) = output.filter(|output| output.status.success()) else {
            return (None, None);
        };
        let value = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        let hardware = value
            .as_ref()
            .and_then(|value| value.get("SPHardwareDataType"))
            .and_then(serde_json::Value::as_array)
            .and_then(|entries| entries.first());
        let field = |name| {
            hardware
                .and_then(|value| value.get(name))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        (field("machine_name"), field("machine_model"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        (None, None)
    }
}

fn benchmark_directory() -> PathBuf {
    let id = Uuid::now_v7().as_simple().to_string();
    PathBuf::from("/tmp").join(format!("amx-bench-{}", &id[id.len() - 12..]))
}

fn percentile_ms(values: &mut [Duration], percentile: f64) -> f64 {
    values.sort_unstable();
    let rank = (percentile * values.len() as f64).ceil() as usize;
    values[rank.saturating_sub(1).min(values.len() - 1)].as_secs_f64() * 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        let mut values = (1..=100).map(Duration::from_millis).collect::<Vec<_>>();
        assert_eq!(percentile_ms(&mut values, 0.95), 95.0);
        assert_eq!(percentile_ms(&mut values, 0.99), 99.0);
    }

    #[test]
    fn budget_requires_integrity_and_every_threshold() {
        let budgets = BenchmarkBudgets::default();
        let measurements = BenchmarkMeasurements {
            cold_startup_p95_ms: budgets.cold_startup_p95_ms,
            broker_idle_rss_mib: budgets.broker_idle_rss_mib,
            in_process_admission_p99_ms: budgets.in_process_admission_p99_ms,
            ipc_admission_p99_ms: budgets.ipc_admission_p99_ms,
            waiter_wakeup_p99_ms: budgets.waiter_wakeup_p99_ms,
            synthetic_event_fan_in_per_second: budgets.synthetic_event_fan_in_per_second,
            synthetic_events_delivered: 10_000,
        };
        assert!(budgets_met(&measurements, &budgets, 10_000));
        assert!(!budgets_met(&measurements, &budgets, 10_001));
    }
}
