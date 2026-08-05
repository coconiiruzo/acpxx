mod support;

use std::time::Duration;

use acpxx::{Broker, CatalogKeyring, CatalogStore, FollowupTask, VersionPolicy, WaitOptions};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};
use support::mock_request;
use uuid::Uuid;

#[tokio::test]
async fn catalog_reload_affects_future_agents_but_not_an_existing_agent_lock() {
    let root = std::env::temp_dir().join(format!("agentmux-runtime-catalog-{}", Uuid::now_v7()));
    let database = root.join("metadata.sqlite3");
    let cache = root.join("compatibility");
    std::fs::create_dir(&root).unwrap();
    let signing_key = SigningKey::from_bytes(&[19_u8; 32]);
    let mut keyring = CatalogKeyring::new();
    keyring
        .insert("runtime-test", signing_key.verifying_key().as_bytes())
        .unwrap();
    let (catalog1, signature1) = signed_catalog(&signing_key, 1);
    let bootstrap = acpxx::VerifiedCatalog::verify(&catalog1, &signature1, &keyring).unwrap();
    let store = CatalogStore::open(&cache, bootstrap, keyring.clone()).unwrap();
    let updater = store.clone();
    let broker = Broker::with_sqlite_options_and_catalog_store(
        2,
        &database,
        store,
        Duration::from_secs(30),
        std::iter::empty(),
    )
    .await
    .unwrap();

    let mut first_request = mock_request("normal", 0.0);
    first_request.version_policy = VersionPolicy::Verified;
    let first = broker.spawn(first_request).await.unwrap();
    let first_receipt = broker
        .wait_run(first.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(
        first_receipt
            .provider_lock
            .as_ref()
            .unwrap()
            .catalog_sequence,
        Some(1)
    );

    let (catalog2, signature2) = signed_catalog(&signing_key, 2);
    updater.install_verified(&catalog2, &signature2).unwrap();
    assert_eq!(broker.reload_compatibility().unwrap().sequence, 2);

    let followup = broker
        .followup(first.agent, first.run.run_id, FollowupTask::new("continue"))
        .await
        .unwrap();
    let followup = broker
        .wait_run(followup, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(followup.provider_lock, first_receipt.provider_lock);

    let mut second_request = mock_request("normal", 0.0);
    second_request.version_policy = VersionPolicy::Verified;
    let second = broker.spawn(second_request).await.unwrap();
    let second = broker
        .wait_run(second.run, WaitOptions::default())
        .await
        .unwrap();
    assert_eq!(
        second.provider_lock.as_ref().unwrap().catalog_sequence,
        Some(2)
    );
    broker.shutdown().await.unwrap();
    drop(broker);
    std::fs::remove_dir_all(root).unwrap();
}

fn signed_catalog(signing_key: &SigningKey, sequence: u64) -> (Vec<u8>, Vec<u8>) {
    let executable =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
    let digest = format!("{:x}", Sha256::digest(std::fs::read(executable).unwrap()));
    let target = acpxx::host_target();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "catalog_id": "agentmux-official",
        "sequence": sequence,
        "generated_at": "2026-08-05T00:00:00Z",
        "expires_at": "2027-08-05T00:00:00Z",
        "entries": [{
            "entry_id": format!("grok/mock/{sequence}"),
            "provider": "grok",
            "target": target,
            "driver_id": "grok-native",
            "driver_revision": 1,
            "agentmux_requirement": ">=2.0.0, <3.0.0",
            "protocol": "v1",
            "identity": {
                "display_version": "0.2.118",
                "normalized_version": "0.2.118",
                "components": {}
            },
            "artifacts": [{
                "subject": "executable",
                "algorithm": "sha256",
                "digest": digest
            }],
            "state": "verified",
            "not_after": null,
            "qualification": {
                "suite_version": 1,
                "tested_at": "2026-08-05T00:00:00Z",
                "evidence_digest": format!("sha256:{:064x}", sequence)
            }
        }],
        "channels": [{
            "provider": "grok",
            "target": target,
            "name": "recommended",
            "entry_id": format!("grok/mock/{sequence}")
        }]
    }))
    .unwrap();
    let signature = signing_key.sign(&bytes);
    let envelope = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "catalog_sha256": format!("{:x}", Sha256::digest(&bytes)),
        "signatures": [{
            "key_id": "runtime-test",
            "algorithm": "ed25519",
            "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
        }]
    }))
    .unwrap();
    (bytes, envelope)
}
