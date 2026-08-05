#[test]
fn tested_provider_document_is_observational_and_runtime_has_no_version_allowlist() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let document = std::fs::read_to_string(root.join("TESTED_PROVIDERS.md"))
        .expect("TESTED_PROVIDERS.md must be checked in");
    assert!(document.contains("non-authoritative"));
    assert!(document.contains("not a runtime allowlist"));

    for driver in acpxx::all_provider_drivers() {
        let serialized = serde_json::to_string(&driver).unwrap();
        assert!(!serialized.contains("tested_version"));
        assert!(!serialized.contains("recommended"));
        assert!(!serialized.contains("catalog"));
    }
}

#[test]
fn central_compatibility_assets_and_workflows_are_absent() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!root.join("compatibility").exists());
    for workflow in [
        "compatibility-publish.yml",
        "provider-candidate-discovery.yml",
        "provider-qualification.yml",
    ] {
        assert!(!root.join(".github/workflows").join(workflow).exists());
    }
}
