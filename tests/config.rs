use std::path::{Path, PathBuf};

use acpxx::config::{ConfigError, ProviderConfig};
use acpxx::{PermissionPolicy, ProviderId, ProviderSpec};
use uuid::Uuid;

#[test]
fn final_v2_profile_resolves_with_optional_local_assertions() {
    let config_path = config_path();
    let executable = fixture_executable();
    write_config(
        &config_path,
        &format!(
            r#"schema_version = 2

[profiles.grok-default]
provider = "grok"
executable = "{}"
permissions = "deny"

[profiles.grok-default.assertions]
version = "9999.0.0"
launch_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
"#,
            executable.display()
        ),
    );
    let profile = ProviderConfig::load(&config_path)
        .unwrap()
        .resolve("grok-default")
        .unwrap();
    assert_eq!(profile.provider, ProviderId::Grok);
    assert_eq!(profile.permission_policy, PermissionPolicy::Deny);
    assert_eq!(profile.assertions.version.as_deref(), Some("9999.0.0"));
    assert_eq!(
        profile.provider_spec,
        ProviderSpec::Grok {
            executable: Some(executable)
        }
    );
}

#[test]
fn floating_profile_has_no_version_gate() {
    let path = config_path();
    write_config(
        &path,
        &format!(
            "schema_version = 2\n[profiles.cursor-default]\nprovider = \"cursor\"\nexecutable = \"{}\"\n",
            fixture_executable().display()
        ),
    );
    let profile = ProviderConfig::load(&path)
        .unwrap()
        .resolve("cursor-default")
        .unwrap();
    assert!(profile.assertions.is_empty());
}

#[test]
fn unknown_fields_and_malformed_assertions_are_rejected() {
    let path = config_path();
    write_config(
        &path,
        &format!(
            "schema_version = 2\n[profiles.grok]\nprovider = \"grok\"\nexecutable = \"{}\"\nauto_update = true\n",
            fixture_executable().display()
        ),
    );
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::Parse(_))
    ));

    write_config(
        &path,
        &format!(
            "schema_version = 2\n[profiles.grok]\nprovider = \"grok\"\nexecutable = \"{}\"\n[profiles.grok.assertions]\nlaunch_sha256 = \"bad\"\n",
            fixture_executable().display()
        ),
    );
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::InvalidProfile { .. })
    ));
}

#[test]
fn relative_executable_is_rejected() {
    let path = config_path();
    write_config(
        &path,
        "schema_version = 2\n[profiles.grok]\nprovider = \"grok\"\nexecutable = \"relative/grok\"\n",
    );
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::InvalidProfile { .. })
    ));
}

#[test]
fn v1_pins_migrate_to_local_assertions_without_catalog() {
    let path = config_path();
    let executable = fixture_executable();
    write_config(
        &path,
        &format!(
            r#"[profiles.grok-default]
provider = "grok"
executable = "{}"
args = ["--no-auto-update", "agent", "stdio"]
version = "0.2.118"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
authentication = "cached"
initialize_verified = true
session_new_verified = true
"#,
            executable.display()
        ),
    );
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::MigrationRequired { found: None })
    ));
    let migration = acpxx::config::check_migration(&path).unwrap();
    assert!(migration.is_ready());
    let rendered = migration.rendered.unwrap();
    assert!(rendered.contains("version = \"0.2.118\""));
    assert!(rendered.contains("launch_sha256"));
    assert!(!rendered.to_ascii_lowercase().contains("catalog"));
}

#[test]
fn catalog_v2_floating_and_exact_profiles_migrate_offline() {
    let path = config_path();
    let executable = fixture_executable();
    write_config(
        &path,
        &format!(
            r#"schema_version = 2

[catalog]
source = "official"

[profiles.grok-floating]
provider = "grok"
executable = "{0}"
version_policy = "verified"

[profiles.grok-exact]
provider = "grok"
executable = "{0}"
version_policy = "exact"
catalog_entry = "grok/0.2.118/aarch64-apple-darwin/sha256-2de5b960"

[profiles.cursor-experimental]
provider = "cursor"
executable = "{0}"
version_policy = "experimental"
"#,
            executable.display()
        ),
    );
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::MigrationRequired { found: Some(2) })
    ));
    let rendered = acpxx::config::check_migration(&path)
        .unwrap()
        .rendered
        .unwrap();
    assert!(!rendered.to_ascii_lowercase().contains("catalog"));
    assert_eq!(
        rendered.matches("[profiles.grok-exact.assertions]").count(),
        1
    );
    assert!(!rendered.contains("[profiles.grok-floating.assertions]"));
    assert!(!rendered.contains("[profiles.cursor-experimental.assertions]"));
}

#[test]
fn unknown_catalog_exact_entry_blocks_migration() {
    let path = config_path();
    let original = format!(
        r#"schema_version = 2
[catalog]
source = "official"
[profiles.grok]
provider = "grok"
executable = "{}"
version_policy = "exact"
catalog_entry = "unknown"
"#,
        fixture_executable().display()
    );
    write_config(&path, &original);
    let migration = acpxx::config::check_migration(&path).unwrap();
    assert!(!migration.is_ready());
    assert!(migration.rendered.is_none());
    assert!(matches!(
        acpxx::config::write_migration(&path),
        Err(ConfigError::MigrationBlocked(_))
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn migration_write_creates_generalized_private_backup() {
    use std::os::unix::fs::PermissionsExt as _;

    let path = config_path();
    write_config(
        &path,
        &format!(
            r#"[profiles.cursor]
provider = "cursor"
executable = "{}"
args = ["acp"]
version = "future-build"
sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
authentication = "login"
initialize_verified = true
session_new_verified = true
"#,
            fixture_executable().display()
        ),
    );
    let backup = acpxx::config::write_migration(&path).unwrap();
    assert!(backup.to_string_lossy().contains("pre-runtime-compat"));
    assert_eq!(
        std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(ProviderConfig::load(&path).is_ok());
}

#[test]
fn oversized_config_is_rejected_before_parsing() {
    let path = config_path();
    std::fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    set_private_permissions(&path);
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::TooLarge)
    ));
}

#[cfg(unix)]
#[test]
fn group_or_world_readable_config_is_rejected() {
    use std::os::unix::fs::PermissionsExt as _;

    let path = config_path();
    std::fs::write(&path, "schema_version = 2\nprofiles = {}\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        ProviderConfig::load(&path),
        Err(ConfigError::InsecurePermissions(_))
    ));
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
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}
