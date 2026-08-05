#![allow(dead_code)]

use std::path::PathBuf;

use acpxx::{PermissionPolicy, ProviderId, ProviderSpec, SpawnRequest, Task};
use uuid::Uuid;

pub fn mock_request(mode: &str, delay: f64) -> SpawnRequest {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
    SpawnRequest {
        provider: ProviderSpec::Grok {
            executable: Some(script),
        },
        cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        task: Task::new(format!(
            "return the fixture output __fake_mode={mode} __fake_delay={delay}"
        )),
        permission_policy: PermissionPolicy::Deny,
        assertions: Default::default(),
    }
}

#[cfg(unix)]
pub struct MockProviderFixture {
    provider: ProviderId,
    executable: PathBuf,
    owned_root: Option<PathBuf>,
}

#[cfg(unix)]
impl MockProviderFixture {
    pub fn new(provider: ProviderId) -> Self {
        Self::new_with_marker(provider, "standard")
    }

    pub fn new_with_marker(provider: ProviderId, marker: &str) -> Self {
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
        let executable = std::env::temp_dir().join(format!(
            "agentmux-mock-{}-{marker}-{}",
            provider.as_str(),
            Uuid::now_v7()
        ));
        std::fs::copy(script, &executable).unwrap();
        Self {
            provider,
            executable,
            owned_root: None,
        }
    }

    pub fn new_with_package_metadata(
        provider: ProviderId,
        marker: &str,
        package: serde_json::Value,
    ) -> Self {
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
        let root = std::env::temp_dir().join(format!(
            "agentmux-mock-package-{}-{}",
            marker,
            Uuid::now_v7()
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = bin.join(format!("agentmux-mock-{}-{marker}", provider.as_str()));
        std::fs::copy(script, &executable).unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::to_vec_pretty(&package).unwrap(),
        )
        .unwrap();
        Self {
            provider,
            executable,
            owned_root: Some(root),
        }
    }

    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }

    pub fn marker_path(&self, suffix: &str) -> PathBuf {
        PathBuf::from(format!("{}{suffix}", self.executable.display()))
    }

    pub fn request(&self, mode: &str) -> SpawnRequest {
        SpawnRequest {
            provider: match self.provider {
                ProviderId::Grok => ProviderSpec::Grok {
                    executable: Some(self.executable.clone()),
                },
                ProviderId::Cursor => ProviderSpec::Cursor {
                    executable: Some(self.executable.clone()),
                },
                ProviderId::Codex => ProviderSpec::Codex {
                    adapter: Some(self.executable.clone()),
                },
                ProviderId::Claude => ProviderSpec::Claude {
                    adapter: Some(self.executable.clone()),
                },
            },
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            task: Task::new(format!("controlled conformance __fake_mode={mode}")),
            permission_policy: PermissionPolicy::Deny,
            assertions: Default::default(),
        }
    }
}

#[cfg(unix)]
impl Drop for MockProviderFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.executable);
        for suffix in [".launched", ".probe-ready", ".probe-continue"] {
            let _ = std::fs::remove_file(self.marker_path(suffix));
        }
        if let Some(root) = &self.owned_root {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

pub fn pinned_provider_spec(
    provider: ProviderId,
    path_variable: &str,
    profile: &str,
) -> ProviderSpec {
    if let Some(executable) = std::env::var_os(path_variable) {
        return match provider {
            ProviderId::Grok => ProviderSpec::Grok {
                executable: Some(PathBuf::from(executable)),
            },
            ProviderId::Cursor => ProviderSpec::Cursor {
                executable: Some(PathBuf::from(executable)),
            },
            ProviderId::Codex => ProviderSpec::Codex {
                adapter: Some(PathBuf::from(executable)),
            },
            ProviderId::Claude => ProviderSpec::Claude {
                adapter: Some(PathBuf::from(executable)),
            },
        };
    }
    let config = acpxx::config::ProviderConfig::load(acpxx::config::default_config_path())
        .unwrap_or_else(|error| {
            panic!(
                "{path_variable} is unset and the pinned provider config could not be loaded: {error}"
            )
        });
    let resolved = config
        .resolve(profile)
        .unwrap_or_else(|error| panic!("failed to resolve profile {profile}: {error}"));
    assert_eq!(resolved.provider, provider);
    resolved.provider_spec
}
