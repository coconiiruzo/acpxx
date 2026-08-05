use std::path::{Path, PathBuf};

use acpxx::config::{ConfigError, ProviderConfig, check_migration, write_migration};
use acpxx::{PermissionPolicy, ProviderId, ProviderSpec, VersionPolicy, bootstrap_catalog};
use uuid::Uuid;

#[test]
fn pinned_profile_resolves_to_the_closed_provider_spec() {
    let config_path = config_path();
    let executable = fixture_executable();
    write_config(
        &config_path,
        &format!(
            r#"
schema_version = 2

[catalog]
source = "official"

[profiles.grok-default]
provider = "grok"
executable = "{}"
version_policy = "verified"
"#,
            executable.display()
        ),
    );

    let config = ProviderConfig::load(&config_path).unwrap();
    let profile = config.resolve("grok-default").unwrap();
    assert_eq!(profile.provider, ProviderId::Grok);
    assert_eq!(profile.permission_policy, PermissionPolicy::Deny);
    assert_eq!(profile.version_policy, VersionPolicy::Verified);
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
schema_version = 2

[catalog]
source = "official"

[profiles.grok-default]
provider = "grok"
executable = "/tmp/grok"
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
fn relative_executable_and_invalid_exact_policy_are_rejected() {
    let config_path = config_path();
    write_config(
        &config_path,
        r#"
schema_version = 2

[catalog]
source = "official"

[profiles.grok-default]
provider = "grok"
executable = "relative/grok"
"#,
    );
    assert!(matches!(
        ProviderConfig::load(&config_path),
        Err(ConfigError::InvalidProfile { .. })
    ));

    let executable = fixture_executable();
    write_config(
        &config_path,
        &format!(
            r#"
schema_version = 2

[catalog]
source = "official"

[profiles.grok-default]
provider = "grok"
executable = "{}"
version_policy = "exact"
"#,
            executable.display()
        ),
    );
    assert!(matches!(
        ProviderConfig::load(&config_path),
        Err(ConfigError::InvalidProfile { .. })
    ));
    let _ = std::fs::remove_file(config_path);
}

#[test]
fn legacy_config_requires_explicit_migration_and_unknown_artifact_is_not_approved() {
    let config_path = config_path();
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
sha256 = "{}"
authentication = "cached login"
initialize_verified = true
session_new_verified = true
"#,
            executable.display(),
            checksum(&executable)
        ),
    );
    assert!(matches!(
        ProviderConfig::load(&config_path),
        Err(ConfigError::MigrationRequired { .. })
    ));
    let bootstrap = bootstrap_catalog().unwrap();
    let migration = check_migration(&config_path, &bootstrap.catalog).unwrap();
    assert!(!migration.is_ready());
    assert!(migration.rendered.is_none());
    assert_eq!(migration.blocking_issues.len(), 1);
    let _ = std::fs::remove_file(config_path);
}

#[test]
fn provider_setup_toml_example_tracks_the_strict_v2_parser() {
    let document = include_str!("../docs/provider-setup.md");
    let example = document
        .split("```toml")
        .nth(1)
        .and_then(|tail| tail.split("```").next())
        .expect("provider setup must contain a TOML example");
    let parsed: ProviderConfig = toml::from_str(example).unwrap();
    assert_eq!(parsed.schema_version, 2);
    assert_eq!(parsed.profiles.len(), 4);
}

#[cfg(unix)]
#[test]
fn all_four_legacy_profiles_migrate_with_backup_and_mode_0600() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = std::env::temp_dir().join(format!("agentmux-config-v2-{}", Uuid::now_v7()));
    std::fs::create_dir_all(root.join("codex/dist")).unwrap();
    std::fs::create_dir_all(root.join("claude/dist")).unwrap();
    let grok = root.join("grok");
    let cursor = root.join("cursor-agent");
    let codex = root.join("codex/dist/index.js");
    let claude = root.join("claude/dist/index.js");
    for path in [&grok, &cursor, &codex, &claude] {
        std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(root.join("codex/package.json"), "{\"name\":\"codex\"}").unwrap();
    std::fs::write(root.join("claude/package.json"), "{\"name\":\"claude\"}").unwrap();
    let config_path = root.join("providers.toml");
    write_config(
        &config_path,
        &format!(
            r#"
[profiles.grok-default]
provider = "grok"
executable = "{}"
args = ["--no-auto-update", "agent", "stdio"]
version = "grok-test"
sha256 = "{}"
authentication = "self-attested"
initialize_verified = true
session_new_verified = true

[profiles.cursor-default]
provider = "cursor"
executable = "{}"
args = ["acp"]
version = "cursor-test"
sha256 = "{}"
authentication = "self-attested"
initialize_verified = true
session_new_verified = true

[profiles.codex-default]
provider = "codex"
adapter_path = "{}"
adapter_version = "codex-acp-test"
bundled_codex_version = "codex-test"
authentication = "self-attested"
initialize_verified = true
session_new_verified = true

[profiles.claude-default]
provider = "claude"
adapter_path = "{}"
adapter_version = "claude-acp-test"
claude_agent_sdk_version = "sdk-test"
authentication = "self-attested"
initialize_verified = true
session_new_verified = true
"#,
            grok.display(),
            checksum(&grok),
            cursor.display(),
            checksum(&cursor),
            codex.display(),
            claude.display(),
        ),
    );
    let mut catalog = bootstrap_catalog().unwrap().catalog;
    let identities = [
        (ProviderId::Grok, "grok-test", None, &grok, None),
        (ProviderId::Cursor, "cursor-test", None, &cursor, None),
        (
            ProviderId::Codex,
            "codex-acp-test",
            Some(("codex", "codex-test")),
            &codex,
            Some(root.join("codex/package.json")),
        ),
        (
            ProviderId::Claude,
            "claude-acp-test",
            Some(("claude_agent_sdk", "sdk-test")),
            &claude,
            Some(root.join("claude/package.json")),
        ),
    ];
    for (provider, version, component, executable, package) in identities {
        let entry = catalog
            .entries
            .iter_mut()
            .find(|entry| entry.provider == provider)
            .unwrap();
        entry.identity.display_version = version.into();
        entry.identity.normalized_version = version.into();
        entry.identity.components = component
            .map(|(name, value)| [(name.to_owned(), value.to_owned())].into_iter().collect())
            .unwrap_or_default();
        entry.artifacts = vec![acpxx::ArtifactDigest {
            subject: "executable".into(),
            algorithm: acpxx::DigestAlgorithm::Sha256,
            digest: checksum(executable),
        }];
        if let Some(package) = package {
            entry.artifacts.push(acpxx::ArtifactDigest {
                subject: "package_metadata".into(),
                algorithm: acpxx::DigestAlgorithm::Sha256,
                digest: checksum(&package),
            });
        }
    }
    let migration = check_migration(&config_path, &catalog).unwrap();
    assert!(migration.is_ready(), "{:?}", migration.blocking_issues);
    assert_eq!(migration.warnings.len(), 4);
    let backup = write_migration(&config_path, &catalog).unwrap();
    assert!(backup.is_file());
    assert_eq!(
        std::fs::metadata(&config_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let migrated = ProviderConfig::load(&config_path).unwrap();
    assert_eq!(migrated.profiles.len(), 4);
    assert!(
        migrated
            .profiles
            .keys()
            .all(|name| { migrated.resolve(name).unwrap().version_policy == VersionPolicy::Exact })
    );
    std::fs::remove_dir_all(root).unwrap();
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
    use sha2::{Digest as _, Sha256};
    format!("{:x}", Sha256::digest(content))
}
