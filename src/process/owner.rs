use agent_client_protocol::{AcpAgent, AcpAgentConfig};

use crate::ProviderManifest;

/// Phase-1 process owner.
///
/// The official ACP SDK starts the child in a dedicated Unix process group and
/// kills/reaps that group when the connection closes. Parent-death supervision
/// and Windows Job Objects remain Phase-3 work.
#[derive(Debug)]
pub struct ProcessTreeOwner {
    manifest: ProviderManifest,
}

impl ProcessTreeOwner {
    #[must_use]
    pub fn new(manifest: ProviderManifest) -> Self {
        Self { manifest }
    }

    #[must_use]
    pub fn startup_timeout(&self) -> std::time::Duration {
        self.manifest.startup_timeout
    }

    #[must_use]
    pub fn into_acp_agent(self) -> AcpAgent {
        let mut config = AcpAgentConfig::new(self.manifest.command).args(self.manifest.args);
        for name in self.manifest.allowed_env {
            if let Ok(value) = std::env::var(&name) {
                config = config.env(name, value);
            }
        }
        AcpAgent::new(config)
    }
}
