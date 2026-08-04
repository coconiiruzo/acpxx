use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use acpxx::{
    Broker, PermissionPolicy, ProviderSpec, SpawnRequest, Task, TerminalRunState, WaitOptions,
};
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "acpxx",
    version,
    about = "Handle-first local coding-agent broker over ACP"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one task through Grok's native ACP endpoint and print a terminal receipt.
    Run {
        /// Workspace sent as the ACP session cwd.
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        /// Override the Grok executable used by the tested manifest.
        #[arg(long)]
        grok: Option<PathBuf>,
        /// Permission policy. The secure default denies mutation requests.
        #[arg(long, value_enum, default_value_t = CliPermissionPolicy::Deny)]
        permissions: CliPermissionPolicy,
        /// Stop waiting after this many seconds. This does not cancel the run.
        #[arg(long)]
        wait_timeout: Option<u64>,
        /// Initial task; all remaining words are joined with spaces.
        #[arg(required = true, trailing_var_arg = true)]
        task: Vec<String>,
    },
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
            eprintln!("acpxx: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> acpxx::Result<ExitCode> {
    match cli.command {
        Command::Run {
            cwd,
            grok,
            permissions,
            wait_timeout,
            task,
        } => {
            let broker = Broker::default();
            let spawned = broker
                .spawn(SpawnRequest {
                    provider: ProviderSpec::Grok { executable: grok },
                    cwd,
                    task: Task::new(task.join(" ")),
                    permission_policy: match permissions {
                        CliPermissionPolicy::Deny => PermissionPolicy::Deny,
                        CliPermissionPolicy::AllowAll => PermissionPolicy::AllowAll,
                    },
                })
                .await?;
            let receipt = broker
                .wait_run(
                    spawned.run,
                    WaitOptions {
                        timeout: wait_timeout.map(Duration::from_secs),
                    },
                )
                .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&receipt)
                    .map_err(|error| acpxx::ControlError::Internal(error.to_string()))?
            );
            Ok(if receipt.state == TerminalRunState::Succeeded {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}
