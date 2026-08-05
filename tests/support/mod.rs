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
    }
}

#[cfg(unix)]
pub struct MockProviderFixture {
    provider: ProviderId,
    executable: PathBuf,
}

#[cfg(unix)]
impl MockProviderFixture {
    pub fn new(provider: ProviderId) -> Self {
        Self::new_with_marker(provider, "standard")
    }

    pub fn new_with_marker(provider: ProviderId, marker: &str) -> Self {
        use std::os::unix::fs::symlink;

        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
        let executable = std::env::temp_dir().join(format!(
            "agentmux-mock-{}-{marker}-{}",
            provider.as_str(),
            Uuid::now_v7()
        ));
        symlink(script, &executable).unwrap();
        Self {
            provider,
            executable,
        }
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
        }
    }
}

#[cfg(unix)]
impl Drop for MockProviderFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.executable);
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
