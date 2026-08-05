use sha2::{Digest as _, Sha256};

#[test]
fn every_bootstrap_entry_points_to_checked_in_qualification_evidence() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let evidence =
        std::fs::read(root.join("compatibility/evidence/bootstrap-2026-08-05.json")).unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(evidence));
    let catalog = acpxx::bootstrap_catalog().unwrap();
    assert!(catalog.catalog.entries.iter().all(|entry| {
        entry.qualification.suite_version == 1 && entry.qualification.evidence_digest == digest
    }));
}

#[test]
fn public_matrix_is_deterministically_rendered_from_the_catalog() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::env::temp_dir().join(format!(
        "agentmux-provider-compatibility-{}.md",
        uuid::Uuid::now_v7()
    ));
    let status = std::process::Command::new("python3")
        .arg(root.join("scripts/render-provider-compatibility.py"))
        .arg(root.join("compatibility/bootstrap/catalog-v1.json"))
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read(&output).unwrap(),
        std::fs::read(root.join("PROVIDER_COMPATIBILITY.md")).unwrap()
    );
    std::fs::remove_file(output).unwrap();
}
