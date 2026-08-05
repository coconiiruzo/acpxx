use std::collections::BTreeSet;

use acpxx::{
    CLAUDE_ACP_TESTED_VERSION, CLAUDE_AGENT_SDK_TESTED_VERSION, CODEX_ACP_TESTED_VERSION,
    CODEX_BUNDLED_TESTED_VERSION, CURSOR_TESTED_VERSION, GROK_TESTED_VERSION,
};

#[test]
fn versioned_compatibility_manifest_matches_compiled_provider_pins() {
    let path = format!(
        "{}/compatibility/agentmux-{}.json",
        env!("CARGO_MANIFEST_DIR"),
        env!("CARGO_PKG_VERSION")
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(manifest["agentmux"], env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest["acp_protocol"], "v1");
    let providers = manifest["providers"].as_object().unwrap();
    assert_eq!(
        providers
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["claude", "codex", "cursor", "grok"])
    );
    assert_eq!(providers["grok"]["tested_version"], GROK_TESTED_VERSION);
    assert_eq!(providers["cursor"]["tested_version"], CURSOR_TESTED_VERSION);
    assert_eq!(
        providers["codex"]["adapter_version"],
        CODEX_ACP_TESTED_VERSION
    );
    assert_eq!(
        providers["codex"]["codex_version"],
        CODEX_BUNDLED_TESTED_VERSION
    );
    assert_eq!(
        providers["claude"]["adapter_version"],
        CLAUDE_ACP_TESTED_VERSION
    );
    assert_eq!(
        providers["claude"]["claude_agent_sdk_version"],
        CLAUDE_AGENT_SDK_TESTED_VERSION
    );
    assert!(
        providers
            .values()
            .all(|provider| provider["status"] == "stable")
    );
}
