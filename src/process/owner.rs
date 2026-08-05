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
            let environment_names = supervisor_environment_names(&self.manifest);
            let supervisor_args = std::iter::once(String::from("__supervise"))
                .chain(
                    environment_names
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

fn supervisor_environment_names(driver: &ProviderDriver) -> Vec<String> {
    let mut names = driver.allowed_env.clone();
    names.extend(driver.fixed_env.keys().cloned());
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_forwards_driver_fixed_environment_names() {
        let driver = crate::codex_driver(None);
        let names = supervisor_environment_names(&driver);
        assert!(names.iter().any(|name| name == "INITIAL_AGENT_MODE"));
        assert!(names.iter().any(|name| name == "CODEX_CONFIG"));
        assert!(names.iter().any(|name| name == "PATH"));
        let unique = names.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), names.len());
    }
}
