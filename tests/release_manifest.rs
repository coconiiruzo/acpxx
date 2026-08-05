use std::collections::BTreeSet;

use acpxx::{CatalogEntryState, ProviderId, all_provider_drivers, bootstrap_catalog};

#[test]
fn signed_bootstrap_catalog_is_the_only_runtime_compatibility_authority() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "2.0.0");
    let verified = bootstrap_catalog().unwrap();
    let providers = verified
        .catalog
        .entries
        .iter()
        .map(|entry| entry.provider)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        providers,
        BTreeSet::from([
            ProviderId::Claude,
            ProviderId::Codex,
            ProviderId::Cursor,
            ProviderId::Grok,
        ])
    );
    assert!(
        verified
            .catalog
            .entries
            .iter()
            .all(|entry| entry.state == CatalogEntryState::Verified)
    );
    for driver in all_provider_drivers() {
        assert!(verified.catalog.entries.iter().any(|entry| {
            entry.provider == driver.id
                && entry.driver_id == driver.driver_id
                && entry.driver_revision == driver.driver_revision
        }));
    }
}

#[test]
fn legacy_v1_manifest_is_historical_and_not_version_selected() {
    let legacy = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("compatibility/legacy/agentmux-1.0.0.json");
    assert!(legacy.is_file());
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("compatibility/agentmux-2.0.0.json")
            .exists()
    );
}
