use std::path::{Path, PathBuf};

use acpxx::config::{ConfigError, ProviderConfig};
use acpxx::{PermissionPolicy, ProviderId, ProviderSpec};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[test]
fn pinned_profile_resolves_to_the_closed_provider_spec() {
    let config_path = config_path();
    let executable = fixture_executable();
    let checksum = checksum(&executable);
    write_config(
        &config_path,
        &format!(
            r#"
[profiles.grok-default]
provider = "grok"
executable = "{}"
args = ["--no-auto-update", "agent", "stdio"]
version = "0.2.118"
sha256 = "{checksum}"
authentication = "cached login"
initialize_verified = true
session_new_verified = true
"#,
            executable.display()
        ),
    );

    let config = ProviderConfig::load(&config_path).unwrap();
    let profile = config.resolve("grok-default").unwrap();
    assert_eq!(profile.provider, ProviderId::Grok);
    assert_eq!(profile.permission_policy, PermissionPolicy::Deny);
    assert_eq!(
        profile.provider_spec,
        ProviderSpec::Grok {
            executable: Some(executable)
        }
    );
    let _ = std::fs::remove_file(config_path);
}

#[test]
fn unknown_fields_cannot_smuggle_secrets_or_update_behavior() {
    let config_path = config_path();
    write_config(
        &config_path,
        r#"
[profiles.grok-default]
provider = "grok"
executable = "/tmp/grok"
args = ["agent", "stdio"]
version = "0.2.118"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
authentication = "cached login"
initialize_verified = true
session_new_verified = true
api_key = "must-not-be-accepted"
auto_update = true
"#,
    );
    assert!(matches!(
        ProviderConfig::load(&config_path),
        Err(ConfigError::Parse(_))
    ));
    let _ = std::fs::remove_file(config_path);
}

#[test]
fn relative_executable_and_checksum_mismatch_are_rejected() {
    let config_path = config_path();
    write_config(
        &config_path,
        r#"
[profiles.grok-default]
provider = "grok"
executable = "relative/grok"
args = ["--no-auto-update", "agent", "stdio"]
version = "0.2.118"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
authentication = "cached login"
initialize_verified = true
session_new_verified = true
"#,
    );
    let config = ProviderConfig::load(&config_path).unwrap();
    assert!(matches!(
        config.resolve("grok-default"),
        Err(ConfigError::InvalidProfile { .. })
    ));

    let executable = fixture_executable();
    write_config(
        &config_path,
        &format!(
            r#"
[profiles.grok-default]
provider = "grok"
executable = "{}"
args = ["--no-auto-update", "agent", "stdio"]
version = "0.2.118"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
authentication = "cached login"
initialize_verified = true
session_new_verified = true
"#,
            executable.display()
        ),
    );
    let config = ProviderConfig::load(&config_path).unwrap();
    assert!(matches!(
        config.resolve("grok-default"),
        Err(ConfigError::InvalidProfile { .. })
    ));
    let _ = std::fs::remove_file(config_path);
}

#[test]
fn oversized_config_is_rejected_before_parsing() {
    let config_path = config_path();
    std::fs::write(&config_path, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    set_private_permissions(&config_path);
    assert!(matches!(
        ProviderConfig::load(&config_path),
        Err(ConfigError::TooLarge)
    ));
    let _ = std::fs::remove_file(config_path);
}

#[cfg(unix)]
#[test]
fn group_or_world_readable_config_is_rejected() {
    use std::os::unix::fs::PermissionsExt;

    let config_path = config_path();
    std::fs::write(&config_path, "profiles = {}").unwrap();
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        ProviderConfig::load(&config_path),
        Err(ConfigError::InsecurePermissions(_))
    ));
    let _ = std::fs::remove_file(config_path);
}

fn fixture_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py")
}

fn config_path() -> PathBuf {
    std::env::temp_dir().join(format!("agentmux-config-{}.toml", Uuid::now_v7()))
}

fn write_config(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
    set_private_permissions(path);
}

fn set_private_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn checksum(path: &Path) -> String {
    let content = std::fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(content))
}
