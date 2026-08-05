use std::collections::BTreeMap;

use acpxx::{
    ArtifactDigest, CatalogEntryState, CatalogError, CatalogKeyring, CatalogSource, CatalogStore,
    CatalogValidationContext, CompatibilityCatalog, CompatibilityLevel, DigestAlgorithm, DriverId,
    ObservedProvider, ResolutionRequest, VerifiedCatalog, VersionPolicy, bootstrap_catalog,
    grok_driver, resolve_provider,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use semver::Version;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

fn valid_catalog() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "catalog_id": "agentmux-official",
        "sequence": 1,
        "generated_at": "2026-08-05T00:00:00Z",
        "expires_at": "2027-08-05T00:00:00Z",
        "entries": [{
            "entry_id": "grok/0.2.118/aarch64-apple-darwin/sha256-test",
            "provider": "grok",
            "target": "aarch64-apple-darwin",
            "driver_id": "grok-native",
            "driver_revision": 1,
            "agentmux_requirement": ">=1.0.0, <3.0.0",
            "protocol": "v1",
            "identity": {
                "display_version": "0.2.118",
                "normalized_version": "0.2.118",
                "components": {}
            },
            "artifacts": [{
                "subject": "executable",
                "algorithm": "sha256",
                "digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }],
            "state": "verified",
            "not_after": null,
            "qualification": {
                "suite_version": 1,
                "tested_at": "2026-08-05T00:00:00Z",
                "evidence_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }
        }],
        "channels": [{
            "provider": "grok",
            "target": "aarch64-apple-darwin",
            "name": "recommended",
            "entry_id": "grok/0.2.118/aarch64-apple-darwin/sha256-test"
        }]
    })
}

#[test]
fn strict_catalog_parses_and_validates_for_a_known_driver() {
    let bytes = serde_json::to_vec(&valid_catalog()).unwrap();
    let catalog = CompatibilityCatalog::from_json(&bytes).unwrap();
    let drivers = BTreeMap::from([(DriverId::new("grok-native"), 1)]);
    catalog
        .validate_for(&CatalogValidationContext {
            target: "aarch64-apple-darwin",
            agentmux_version: &Version::parse("2.0.0").unwrap(),
            drivers: &drivers,
            now: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
        })
        .unwrap();
}

#[test]
fn unknown_fields_and_ambiguous_channels_are_rejected() {
    let mut unknown = valid_catalog();
    unknown["command"] = serde_json::json!("malicious-provider");
    assert!(matches!(
        CompatibilityCatalog::from_json(&serde_json::to_vec(&unknown).unwrap()),
        Err(CatalogError::Parse(_))
    ));

    let mut duplicate = valid_catalog();
    duplicate["channels"] = serde_json::json!([
        duplicate["channels"][0].clone(),
        duplicate["channels"][0].clone()
    ]);
    assert!(matches!(
        CompatibilityCatalog::from_json(&serde_json::to_vec(&duplicate).unwrap()),
        Err(CatalogError::Invalid(_))
    ));
}

#[test]
fn blocked_entries_cannot_be_recommended() {
    let mut value = valid_catalog();
    value["entries"][0]["state"] = serde_json::json!("blocked");
    let result = CompatibilityCatalog::from_json(&serde_json::to_vec(&value).unwrap());
    assert!(matches!(result, Err(CatalogError::Invalid(_))));
    assert_eq!(
        serde_json::from_value::<CatalogEntryState>(serde_json::json!("blocked")).unwrap(),
        CatalogEntryState::Blocked
    );
}

#[test]
fn exact_bytes_require_a_trusted_ed25519_signature() {
    let catalog = serde_json::to_vec(&valid_catalog()).unwrap();
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let signature = signing_key.sign(&catalog);
    let digest = format!("{:x}", Sha256::digest(&catalog));
    let envelope = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "catalog_sha256": digest,
        "signatures": [{
            "key_id": "test-key",
            "algorithm": "ed25519",
            "signature": STANDARD.encode(signature.to_bytes())
        }]
    }))
    .unwrap();
    let mut keyring = CatalogKeyring::new();
    keyring
        .insert("test-key", signing_key.verifying_key().as_bytes())
        .unwrap();

    let verified = VerifiedCatalog::verify(&catalog, &envelope, &keyring).unwrap();
    assert_eq!(verified.catalog.sequence, 1);
    assert_eq!(verified.signer_key_ids, ["test-key"]);

    let mut tampered = catalog;
    tampered.push(b' ');
    assert!(matches!(
        VerifiedCatalog::verify(&tampered, &envelope, &keyring),
        Err(CatalogError::Signature(_))
    ));
}

#[test]
fn cache_install_is_atomic_monotonic_and_reopenable() {
    let fixture = SignedCatalogFixture::new();
    let root = std::env::temp_dir().join(format!("agentmux-catalog-test-{}", uuid::Uuid::now_v7()));
    let (bootstrap_bytes, bootstrap_signature) = fixture.signed(1);
    let bootstrap =
        VerifiedCatalog::verify(&bootstrap_bytes, &bootstrap_signature, &fixture.keyring).unwrap();
    let store = CatalogStore::open(&root, bootstrap.clone(), fixture.keyring.clone()).unwrap();
    assert_eq!(store.snapshot().source, CatalogSource::Bootstrap);

    let (next_bytes, next_signature) = fixture.signed(2);
    let installed = store
        .install_verified(&next_bytes, &next_signature)
        .unwrap();
    assert_eq!(installed.source, CatalogSource::Cache);
    assert_eq!(installed.catalog.catalog.sequence, 2);
    assert!(matches!(
        store.install_verified(&next_bytes, &next_signature),
        Err(CatalogError::Rollback {
            candidate: 2,
            accepted: 2
        })
    ));

    let reopened = CatalogStore::open(&root, bootstrap, fixture.keyring.clone()).unwrap();
    assert_eq!(reopened.snapshot().catalog.catalog.sequence, 2);
    std::fs::write(root.join("state.json"), b"corrupt-primary-state").unwrap();
    let recovered = CatalogStore::open(
        &root,
        recovered_bootstrap(&fixture),
        fixture.keyring.clone(),
    )
    .unwrap();
    assert_eq!(recovered.snapshot().catalog.catalog.sequence, 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn expired_catalog_update_is_rejected_without_changing_active_state() {
    let fixture = SignedCatalogFixture::new();
    let root = std::env::temp_dir().join(format!("agentmux-catalog-test-{}", uuid::Uuid::now_v7()));
    let bootstrap = recovered_bootstrap(&fixture);
    let store = CatalogStore::open(&root, bootstrap, fixture.keyring.clone()).unwrap();
    let mut expired = valid_catalog();
    expired["sequence"] = serde_json::json!(2);
    expired["generated_at"] = serde_json::json!("2024-01-01T00:00:00Z");
    expired["expires_at"] = serde_json::json!("2025-01-01T00:00:00Z");
    let (bytes, signature) = fixture.signed_value(expired);
    assert!(matches!(
        store.install_verified(&bytes, &signature),
        Err(CatalogError::Expired(_))
    ));
    assert_eq!(store.snapshot().catalog.catalog.sequence, 1);
}

#[test]
fn resolver_is_exact_deny_first_and_experimental_is_explicit() {
    let fixture = SignedCatalogFixture::new();
    let (bytes, signature) = fixture.signed(1);
    let catalog = VerifiedCatalog::verify(&bytes, &signature, &fixture.keyring).unwrap();
    let driver = grok_driver(Some("/tmp/grok".into()));
    let observed = ObservedProvider {
        identity: catalog.catalog.entries[0].identity.clone(),
        artifacts: catalog.catalog.entries[0].artifacts.clone(),
        executable: "/tmp/grok".into(),
        executable_identity: acpxx::ExecutableFileIdentity {
            owner: 0,
            mode: 0,
            device: 0,
            inode: 0,
            size: 0,
            modified_seconds: 0,
            modified_nanoseconds: 0,
        },
        qualified_files: Vec::new(),
    };
    let version = Version::parse("2.0.0").unwrap();
    let request = ResolutionRequest {
        driver: &driver,
        observed: &observed,
        target: "aarch64-apple-darwin",
        policy: VersionPolicy::Verified,
        exact_entry: None,
        agentmux_version: &version,
        now: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
    };
    let resolved = resolve_provider(&catalog, &request).unwrap();
    assert_eq!(resolved.compatibility, CompatibilityLevel::Verified);
    assert_eq!(resolved.catalog_sequence, Some(1));

    let unknown = ObservedProvider {
        identity: observed.identity.clone(),
        artifacts: vec![ArtifactDigest {
            subject: "executable".into(),
            algorithm: DigestAlgorithm::Sha256,
            digest: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
        }],
        executable: "/tmp/grok".into(),
        executable_identity: observed.executable_identity.clone(),
        qualified_files: Vec::new(),
    };
    let experimental = resolve_provider(
        &catalog,
        &ResolutionRequest {
            observed: &unknown,
            policy: VersionPolicy::Experimental,
            ..request
        },
    )
    .unwrap();
    assert_eq!(experimental.compatibility, CompatibilityLevel::Experimental);
    assert!(experimental.catalog_entry_id.is_none());

    let mut blocked_value = valid_catalog();
    blocked_value["entries"][0]["state"] = serde_json::json!("blocked");
    blocked_value["entries"][0]["agentmux_requirement"] = serde_json::json!(">=9.0.0");
    blocked_value["channels"] = serde_json::json!([]);
    let (blocked_bytes, blocked_signature) = fixture.signed_value(blocked_value);
    let blocked =
        VerifiedCatalog::verify(&blocked_bytes, &blocked_signature, &fixture.keyring).unwrap();
    assert!(matches!(
        resolve_provider(
            &blocked,
            &ResolutionRequest {
                driver: &driver,
                observed: &observed,
                target: "aarch64-apple-darwin",
                policy: VersionPolicy::Experimental,
                exact_entry: None,
                agentmux_version: &version,
                now: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
            }
        ),
        Err(CatalogError::Resolution(message)) if message.contains("blocked")
    ));
}

#[test]
fn checked_in_bootstrap_catalog_has_a_valid_official_signature() {
    let bootstrap = bootstrap_catalog().unwrap();
    assert_eq!(bootstrap.catalog.catalog_id, "agentmux-official");
    assert_eq!(bootstrap.catalog.sequence, 1);
    assert_eq!(bootstrap.catalog.entries.len(), 4);
    assert_eq!(bootstrap.signer_key_ids, ["agentmux-catalog-2026-01"]);
}

#[test]
fn signed_file_source_is_verified_without_installing_a_cache_generation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let store = CatalogStore::open_file(
        root.join("compatibility/bootstrap/catalog-v1.json"),
        root.join("compatibility/bootstrap/catalog-v1.sig"),
        acpxx::bootstrap_catalog().unwrap(),
        acpxx::official_keyring().unwrap(),
    )
    .unwrap();
    assert_eq!(store.status().source, acpxx::CatalogSource::File);
    assert!(!store.status().lkg_available);
    assert_eq!(store.reload().unwrap().catalog.catalog.sequence, 1);
}

struct SignedCatalogFixture {
    signing_key: SigningKey,
    keyring: CatalogKeyring,
}

impl SignedCatalogFixture {
    fn new() -> Self {
        let signing_key = SigningKey::from_bytes(&[11_u8; 32]);
        let mut keyring = CatalogKeyring::new();
        keyring
            .insert("cache-test-key", signing_key.verifying_key().as_bytes())
            .unwrap();
        Self {
            signing_key,
            keyring,
        }
    }

    fn signed(&self, sequence: u64) -> (Vec<u8>, Vec<u8>) {
        let mut value = valid_catalog();
        value["sequence"] = serde_json::json!(sequence);
        self.signed_value(value)
    }

    fn signed_value(&self, value: serde_json::Value) -> (Vec<u8>, Vec<u8>) {
        let catalog = serde_json::to_vec(&value).unwrap();
        let signature = self.signing_key.sign(&catalog);
        let envelope = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "catalog_sha256": format!("{:x}", Sha256::digest(&catalog)),
            "signatures": [{
                "key_id": "cache-test-key",
                "algorithm": "ed25519",
                "signature": STANDARD.encode(signature.to_bytes())
            }]
        }))
        .unwrap();
        (catalog, envelope)
    }
}

fn recovered_bootstrap(fixture: &SignedCatalogFixture) -> VerifiedCatalog {
    let (bytes, signature) = fixture.signed(1);
    VerifiedCatalog::verify(&bytes, &signature, &fixture.keyring).unwrap()
}
