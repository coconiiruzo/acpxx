use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use acpxx::ipc::{IpcClient, IpcCommand, IpcResponse, LocalServer, default_socket_path};
use acpxx::{
    AgentHandle, AgentId, AgentMessage, Broker, FollowupTask, ListQuery, PermissionPolicy,
    ProviderId, ProviderSpec, RunHandle, RunId, SpawnRequest, Task,
};
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "agentmux",
    version,
    about = "Handle-first local coding-agent broker over ACP"
)]
struct Cli {
    /// Broker Unix Domain Socket.
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    /// Non-secret provider profile configuration with optional local assertions.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the local broker until SIGINT or SIGTERM.
    Serve {
        #[arg(long, default_value_t = 8)]
        max_concurrency: usize,
        #[arg(long, default_value_t = 30 * 60)]
        idle_ttl_secs: u64,
        /// Per-provider Run limit in `provider=count` form. May be repeated.
        #[arg(long = "provider-limit", value_parser = parse_provider_limit)]
        provider_limits: Vec<(ProviderId, usize)>,
        #[arg(long)]
        database: Option<PathBuf>,
    },
    /// Create an Agent, provider session, and initial Run.
    Spawn {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_enum, default_value_t = CliProvider::Grok)]
        provider: CliProvider,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        /// Explicit provider/adapter executable. No auto-install is performed.
        #[arg(long)]
        executable: Option<PathBuf>,
        #[arg(long, value_enum)]
        permissions: Option<CliPermissionPolicy>,
        /// Interrupt this Run after the execution deadline.
        #[arg(long)]
        deadline_ms: Option<u64>,
        #[arg(required = true, trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// Queue a message without starting or steering a Run.
    Send {
        agent: AgentId,
        #[arg(required = true, trailing_var_arg = true)]
        message: Vec<String>,
    },
    /// Start the next Run on the Agent's existing ACP session.
    Followup {
        agent: AgentId,
        #[arg(long)]
        after: RunId,
        #[arg(long)]
        deadline_ms: Option<u64>,
        #[arg(required = true, trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// Interrupt a Run. The owning Agent is resolved from the broker registry.
    Interrupt { run: RunId },
    /// List Agent, Run, and provider snapshots.
    List {
        #[arg(long)]
        agent: Option<AgentId>,
        #[arg(long)]
        run: Option<RunId>,
        #[arg(long)]
        provider: Option<ProviderId>,
    },
    /// Wait for one Run without cancelling it on client timeout/disconnect.
    Wait {
        run: RunId,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Wait for the first terminal Run without cancelling the others.
    WaitAny {
        #[arg(required = true, num_args = 1..)]
        runs: Vec<RunId>,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Wait for all Runs and return receipts in input order.
    WaitAll {
        #[arg(required = true, num_args = 1..)]
        runs: Vec<RunId>,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Stream live Run events as JSON Lines, followed by the terminal receipt.
    Watch { run: RunId },
    /// Inspect local provider artifacts without opening an ACP session.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Validate or migrate provider profile configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Inspect versions, authentication, IPC permissions, and SQLite schema.
    Doctor {
        #[arg(long)]
        database: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Measure Phase 17 broker overhead independently from provider/model time.
    Benchmark {
        #[arg(long, default_value_t = 100)]
        samples: usize,
        #[arg(long, default_value_t = 100_000)]
        events: usize,
        /// Fail when a release macOS arm64 measurement exceeds a frozen budget.
        #[arg(long)]
        enforce: bool,
        #[arg(long)]
        json: bool,
    },
    /// Direct single-process smoke path retained for provider diagnostics.
    Run {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long)]
        grok: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CliPermissionPolicy::Deny)]
        permissions: CliPermissionPolicy,
        #[arg(long)]
        deadline_ms: Option<u64>,
        #[arg(required = true, trailing_var_arg = true)]
        task: Vec<String>,
    },
    #[command(name = "__supervise", hide = true)]
    Supervise {
        #[arg(long = "allow-env")]
        allow_env: Vec<String>,
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    /// Observe executable safety, identity, digest, and local assertion result.
    Inspect {
        profile: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Migrate legacy provider configuration to final schema v2.
    Migrate {
        #[arg(long, conflicts_with = "write")]
        check: bool,
        #[arg(long, conflicts_with = "check")]
        write: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliProvider {
    Codex,
    Claude,
    Grok,
    Cursor,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliPermissionPolicy {
    Deny,
    AllowAll,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("agentmux: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let socket = cli.socket.unwrap_or_else(default_socket_path);
    let config = cli
        .config
        .unwrap_or_else(acpxx::config::default_config_path);
    match cli.command {
        Command::Supervise { allow_env, command } => {
            let status = acpxx::process::supervise(command, &allow_env).await?;
            Ok(exit_for(status.success()))
        }
        Command::Serve {
            max_concurrency,
            idle_ttl_secs,
            provider_limits,
            database,
        } => {
            let server = LocalServer::bind(&socket)?;
            let shutdown = async {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{SignalKind, signal};
                    let mut terminate = signal(SignalKind::terminate())
                        .expect("SIGTERM handler must be installable");
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {},
                        _ = terminate.recv() => {},
                    }
                }
            };
            let database = database.unwrap_or_else(acpxx::doctor::default_database_path);
            let broker = Broker::with_sqlite_options(
                max_concurrency,
                database,
                Duration::from_secs(idle_ttl_secs),
                provider_limits,
            )
            .await?;
            server.serve_until(broker, shutdown).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            cwd,
            grok,
            permissions,
            deadline_ms,
            task,
        } => {
            let broker = Broker::default();
            let spawned = broker
                .spawn(SpawnRequest {
                    provider: ProviderSpec::Grok { executable: grok },
                    cwd,
                    task: task_with_deadline(task.join(" "), deadline_ms),
                    permission_policy: permission_policy(permissions),
                    assertions: Default::default(),
                })
                .await?;
            let receipt = broker
                .wait_run(spawned.run, acpxx::WaitOptions::default())
                .await?;
            broker.shutdown().await?;
            print_json(&receipt)?;
            Ok(exit_for(
                receipt.state == acpxx::TerminalRunState::Succeeded,
            ))
        }
        Command::Provider {
            command: ProviderCommand::Inspect { profile, json },
        } => {
            let resolved = acpxx::config::ProviderConfig::load(&config)?.resolve(&profile)?;
            let driver = resolved.provider_spec.driver()?;
            let observed = acpxx::acp::observe_provider(&driver)
                .await
                .map_err(|failure| failure.message)?;
            let assertion_result = observed.assertion_result(&resolved.assertions);
            let identity = acpxx::ProviderExecutionIdentity {
                provider: driver.id,
                driver_id: driver.driver_id,
                driver_revision: driver.driver_revision,
                target: acpxx::host_target().into(),
                executable_path: observed.executable,
                launch_sha256: observed.launch_sha256,
                observed_version: observed.version,
                observed_components: observed.components,
                acp_protocol_version: None,
                acp_agent_info: None,
                capability_digest: None,
                assertion_result: assertion_result.clone(),
            };
            if json {
                print_json(&identity)?;
            } else {
                println!("provider\t{}", identity.provider);
                println!(
                    "driver\t{}@{}",
                    identity.driver_id.0, identity.driver_revision
                );
                println!("executable\t{}", identity.executable_path.display());
                println!("launch_sha256\t{}", identity.launch_sha256);
                println!("observed_version\t{:?}", identity.observed_version);
                println!("assertions\t{:?}", identity.assertion_result);
            }
            Ok(exit_for(!matches!(
                assertion_result,
                acpxx::AssertionResult::Failed { .. }
            )))
        }
        Command::Config {
            command: ConfigCommand::Migrate { check, write, json },
        } => {
            let migration = acpxx::config::check_migration(&config)?;
            let ready = migration.is_ready();
            if write {
                let backup = acpxx::config::write_migration(&config)?;
                if json {
                    print_json(&serde_json::json!({
                        "ready": true,
                        "written": true,
                        "backup": backup,
                        "warnings": migration.warnings,
                    }))?;
                } else {
                    println!("migrated {}", config.display());
                    println!("backup {}", backup.display());
                    for warning in migration.warnings {
                        println!("warning: {warning}");
                    }
                }
            } else if json {
                print_json(&migration)?;
            } else {
                println!("ready\t{ready}");
                for issue in migration.blocking_issues {
                    println!("blocking\t{issue}");
                }
                for warning in migration.warnings {
                    println!("warning\t{warning}");
                }
                if !check {
                    println!("hint\tuse --write to replace the config after backup");
                }
            }
            Ok(exit_for(ready))
        }
        Command::Doctor { database, json } => {
            let report = acpxx::doctor::inspect_with_config(
                &socket,
                &database.unwrap_or_else(acpxx::doctor::default_database_path),
                &config,
            )
            .await;
            if json {
                print_json(&report)?;
            } else {
                for check in &report.checks {
                    println!("{:?}\t{}\t{}", check.status, check.name, check.message);
                }
            }
            Ok(exit_for(report.healthy))
        }
        Command::Benchmark {
            samples,
            events,
            enforce,
            json,
        } => {
            let report =
                acpxx::benchmark::run(acpxx::benchmark::BenchmarkOptions { samples, events })
                    .await?;
            if json {
                print_json(&report)?;
            } else {
                println!(
                    "cold_startup_p95_ms\t{:.3}",
                    report.measurements.cold_startup_p95_ms
                );
                println!(
                    "broker_idle_rss_mib\t{:.3}",
                    report.measurements.broker_idle_rss_mib
                );
                println!(
                    "in_process_admission_p99_ms\t{:.3}",
                    report.measurements.in_process_admission_p99_ms
                );
                println!(
                    "ipc_admission_p99_ms\t{:.3}",
                    report.measurements.ipc_admission_p99_ms
                );
                println!(
                    "waiter_wakeup_p99_ms\t{:.3}",
                    report.measurements.waiter_wakeup_p99_ms
                );
                println!(
                    "synthetic_event_fan_in_per_second\t{:.0}",
                    report.measurements.synthetic_event_fan_in_per_second
                );
            }
            if enforce && !report.qualification_eligible {
                return Err("budget enforcement requires a release macOS arm64 build".into());
            }
            Ok(exit_for(!enforce || report.passed == Some(true)))
        }
        command => run_client_command(&IpcClient::new(socket), command, &config).await,
    }
}

async fn run_client_command(
    client: &IpcClient,
    command: Command,
    config_path: &std::path::Path,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match command {
        Command::Spawn {
            profile,
            provider,
            cwd,
            executable,
            permissions,
            deadline_ms,
            task,
        } => {
            let (provider, profile_permissions, assertions) = match profile {
                Some(profile) => {
                    let profile =
                        acpxx::config::ProviderConfig::load(config_path)?.resolve(&profile)?;
                    (
                        profile.provider_spec,
                        profile.permission_policy,
                        profile.assertions,
                    )
                }
                None => (
                    provider_spec(provider, executable),
                    PermissionPolicy::Deny,
                    Default::default(),
                ),
            };
            let response = client
                .request(IpcCommand::Spawn(SpawnRequest {
                    provider,
                    cwd,
                    task: task_with_deadline(task.join(" "), deadline_ms),
                    permission_policy: permissions
                        .map(permission_policy)
                        .unwrap_or(profile_permissions),
                    assertions,
                }))
                .await?;
            print_response(response)
        }
        Command::Send { agent, message } => {
            let response = client
                .request(IpcCommand::Send {
                    agent: AgentHandle { agent_id: agent },
                    message: AgentMessage {
                        content: message.join(" "),
                    },
                })
                .await?;
            print_response(response)
        }
        Command::Followup {
            agent,
            after,
            deadline_ms,
            task,
        } => {
            let task = match deadline_ms {
                Some(milliseconds) => FollowupTask::new(task.join(" "))
                    .with_deadline(Duration::from_millis(milliseconds)),
                None => FollowupTask::new(task.join(" ")),
            };
            let response = client
                .request(IpcCommand::Followup {
                    agent: AgentHandle { agent_id: agent },
                    after,
                    task,
                })
                .await?;
            print_response(response)
        }
        Command::Interrupt { run } => {
            let handle = resolve_run(client, run).await?;
            print_response(client.request(IpcCommand::Interrupt(handle)).await?)
        }
        Command::List {
            agent,
            run,
            provider,
        } => print_response(
            client
                .request(IpcCommand::List(ListQuery {
                    agent_id: agent,
                    run_id: run,
                    provider,
                }))
                .await?,
        ),
        Command::Wait { run, timeout_ms } => {
            let handle = resolve_run(client, run).await?;
            print_response(
                client
                    .request(IpcCommand::WaitRun {
                        run: handle,
                        timeout_ms,
                    })
                    .await?,
            )
        }
        Command::WaitAny { runs, timeout_ms } => {
            let handles = resolve_runs(client, runs).await?;
            print_response(
                client
                    .request(IpcCommand::WaitAny {
                        runs: handles,
                        timeout_ms,
                    })
                    .await?,
            )
        }
        Command::WaitAll { runs, timeout_ms } => {
            let handles = resolve_runs(client, runs).await?;
            print_response(
                client
                    .request(IpcCommand::WaitAll {
                        runs: handles,
                        timeout_ms,
                    })
                    .await?,
            )
        }
        Command::Watch { run } => {
            let handle = resolve_run(client, run).await?;
            let mut responses = client.watch(handle).await?;
            while let Some(response) = responses.recv().await {
                let terminal =
                    matches!(response, IpcResponse::StreamEnd(_) | IpcResponse::Error(_));
                println!("{}", serde_json::to_string(&response)?);
                if terminal {
                    return Ok(exit_for(!matches!(response, IpcResponse::Error(_))));
                }
            }
            Ok(ExitCode::FAILURE)
        }
        Command::Serve { .. }
        | Command::Run { .. }
        | Command::Provider { .. }
        | Command::Config { .. }
        | Command::Doctor { .. }
        | Command::Benchmark { .. }
        | Command::Supervise { .. } => {
            unreachable!("local-only commands are handled before IPC dispatch")
        }
    }
}

async fn resolve_runs(
    client: &IpcClient,
    run_ids: Vec<RunId>,
) -> Result<Vec<RunHandle>, Box<dyn std::error::Error>> {
    let mut handles = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        handles.push(resolve_run(client, run_id).await?);
    }
    Ok(handles)
}

async fn resolve_run(
    client: &IpcClient,
    run_id: RunId,
) -> Result<RunHandle, Box<dyn std::error::Error>> {
    match client
        .request(IpcCommand::List(ListQuery {
            run_id: Some(run_id),
            ..ListQuery::default()
        }))
        .await?
    {
        IpcResponse::List(snapshot) => snapshot
            .runs
            .first()
            .map(|run| RunHandle {
                agent_id: run.agent_id,
                run_id,
            })
            .ok_or_else(|| format!("run not found: {run_id}").into()),
        IpcResponse::Error(error) => Err(format!("{}: {}", error.code, error.message).into()),
        response => Err(format!("unexpected broker response: {response:?}").into()),
    }
}

fn print_response(response: IpcResponse) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let success = !matches!(response, IpcResponse::Error(_));
    print_json(&response)?;
    Ok(exit_for(success))
}

fn print_json(value: &impl serde::Serialize) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

const fn exit_for(success: bool) -> ExitCode {
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

const fn permission_policy(value: CliPermissionPolicy) -> PermissionPolicy {
    match value {
        CliPermissionPolicy::Deny => PermissionPolicy::Deny,
        CliPermissionPolicy::AllowAll => PermissionPolicy::AllowAll,
    }
}

fn parse_provider_limit(value: &str) -> Result<(ProviderId, usize), String> {
    let (provider, limit) = value
        .split_once('=')
        .ok_or_else(|| "expected provider=count".to_owned())?;
    let provider = provider
        .parse::<ProviderId>()
        .map_err(|error| error.to_string())?;
    let limit = limit
        .parse::<usize>()
        .map_err(|error| format!("invalid provider limit: {error}"))?;
    if limit == 0 {
        return Err("provider limit must be greater than zero".into());
    }
    Ok((provider, limit))
}

fn provider_spec(provider: CliProvider, executable: Option<PathBuf>) -> ProviderSpec {
    match provider {
        CliProvider::Grok => ProviderSpec::Grok { executable },
        CliProvider::Cursor => ProviderSpec::Cursor { executable },
        CliProvider::Codex => ProviderSpec::Codex {
            adapter: executable,
        },
        CliProvider::Claude => ProviderSpec::Claude {
            adapter: executable,
        },
    }
}

fn task_with_deadline(content: String, deadline_ms: Option<u64>) -> Task {
    match deadline_ms {
        Some(milliseconds) => Task::new(content).with_deadline(Duration::from_millis(milliseconds)),
        None => Task::new(content),
    }
}
