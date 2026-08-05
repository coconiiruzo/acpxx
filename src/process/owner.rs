use agent_client_protocol::{AcpAgent, AcpAgentConfig};

use crate::ProviderDriver;

/// Builds the owned ACP process endpoint.
///
/// The distributed `agentmux` binary wraps providers in its hidden supervisor
/// mode. Test binaries use the SDK's process-group ownership directly.
#[derive(Debug)]
pub struct ProcessTreeOwner {
    manifest: ProviderDriver,
}

impl ProcessTreeOwner {
    #[must_use]
    pub fn new(manifest: ProviderDriver) -> Self {
        Self { manifest }
    }

    #[must_use]
    pub fn startup_timeout(&self) -> std::time::Duration {
        self.manifest.startup_timeout
    }

    #[must_use]
    pub fn into_acp_agent(self) -> AcpAgent {
        let mut provider_command = vec![self.manifest.command.to_string_lossy().into_owned()];
        provider_command.extend(self.manifest.args.iter().cloned());
        let executable = std::env::current_exe().ok();
        let use_supervisor = executable.as_ref().is_some_and(|path| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "agentmux")
        });
        let mut config = if let Some(executable) = executable.filter(|_| use_supervisor) {
            let supervisor_args = std::iter::once(String::from("__supervise"))
                .chain(
                    self.manifest
                        .allowed_env
                        .iter()
                        .flat_map(|name| [String::from("--allow-env"), name.clone()]),
                )
                .chain(std::iter::once(String::from("--")))
                .chain(provider_command)
                .collect::<Vec<_>>();
            AcpAgentConfig::new(executable).args(supervisor_args)
        } else {
            AcpAgentConfig::new(self.manifest.command).args(self.manifest.args)
        };
        for name in self.manifest.allowed_env {
            if let Ok(value) = std::env::var(&name) {
                config = config.env(name, value);
            }
        }
        for (name, value) in self.manifest.fixed_env {
            config = config.env(name, value);
        }
        AcpAgent::new(config)
    }
}
