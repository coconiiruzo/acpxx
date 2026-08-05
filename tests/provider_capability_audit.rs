mod support;

use std::time::Duration;

use acpxx::{Broker, ListQuery, PermissionPolicy, ProviderId, SpawnRequest, Task, WaitOptions};
use support::pinned_provider_spec;

#[derive(Debug, Eq, PartialEq)]
struct StableCapabilities {
    load_session: bool,
    image: bool,
    audio: bool,
    embedded_context: bool,
    mcp_http: bool,
    mcp_sse: bool,
    session_methods: Vec<&'static str>,
    auth_logout: bool,
}

#[tokio::test]
#[ignore = "queries authenticated provider capabilities and may consume model quota"]
async fn authenticated_provider_capability_inventory() {
    let providers = [
        (ProviderId::Grok, "AGENTMUX_GROK_PATH", "grok-default"),
        (ProviderId::Cursor, "AGENTMUX_CURSOR_PATH", "cursor-default"),
        (
            ProviderId::Codex,
            "AGENTMUX_CODEX_ACP_PATH",
            "codex-default",
        ),
        (
            ProviderId::Claude,
            "AGENTMUX_CLAUDE_ACP_PATH",
            "claude-default",
        ),
    ];
    let selected = std::env::var("AGENTMUX_REAL_PROVIDER").ok();
    for (provider, variable, profile) in providers {
        if selected
            .as_deref()
            .is_some_and(|value| value != provider.as_str())
        {
            continue;
        }
        let broker = Broker::new(1);
        let spawned = broker
            .spawn(SpawnRequest {
                provider: pinned_provider_spec(provider, variable, profile),
                cwd: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                task: Task::new("Do not use tools. Reply with exactly: capability-audit-ok")
                    .with_deadline(Duration::from_secs(120)),
                permission_policy: PermissionPolicy::Deny,
                version_policy: acpxx::VersionPolicy::Verified,
                catalog_entry: None,
                allow_unverified_mutations: false,
            })
            .await
            .unwrap();
        let receipt = broker
            .wait_run(
                spawned.run,
                WaitOptions {
                    timeout: Some(Duration::from_secs(130)),
                },
            )
            .await
            .unwrap();
        assert!(receipt.failure.is_none(), "{provider}: {receipt:?}");
        let snapshot = broker
            .list(ListQuery {
                agent_id: Some(spawned.agent.agent_id),
                ..ListQuery::default()
            })
            .await
            .unwrap();
        let capabilities = snapshot.agents[0]
            .provider_capabilities
            .as_ref()
            .expect("initialize must publish a capability snapshot");
        assert!(capabilities.is_object(), "{provider}: {capabilities}");
        assert_eq!(
            stable_capabilities(capabilities),
            expected_capabilities(provider),
            "{provider}: {}",
            serde_json::to_string(capabilities).unwrap()
        );
        let broker_capabilities = &snapshot.agents[0].broker_capabilities;
        assert!(broker_capabilities.text_prompt);
        assert!(broker_capabilities.event_stream);
        assert!(broker_capabilities.permission_response);
        assert!(broker_capabilities.filesystem_host);
        assert!(broker_capabilities.terminal_host);
        assert!(!broker_capabilities.image_prompt);
        assert!(!broker_capabilities.audio_prompt);
        assert!(!broker_capabilities.embedded_context);
        assert!(!broker_capabilities.mcp_servers);
        assert!(!broker_capabilities.session_load);
        assert!(!broker_capabilities.session_resume);
        broker.shutdown().await.unwrap();
    }
}

fn stable_capabilities(value: &serde_json::Value) -> StableCapabilities {
    let session = value["sessionCapabilities"].as_object().unwrap();
    let mut session_methods = ["list", "delete", "additionalDirectories", "resume", "close"]
        .into_iter()
        .filter(|name| session.contains_key(*name))
        .collect::<Vec<_>>();
    session_methods.sort_unstable();
    StableCapabilities {
        load_session: value["loadSession"].as_bool().unwrap_or(false),
        image: value["promptCapabilities"]["image"]
            .as_bool()
            .unwrap_or(false),
        audio: value["promptCapabilities"]["audio"]
            .as_bool()
            .unwrap_or(false),
        embedded_context: value["promptCapabilities"]["embeddedContext"]
            .as_bool()
            .unwrap_or(false),
        mcp_http: value["mcpCapabilities"]["http"].as_bool().unwrap_or(false),
        mcp_sse: value["mcpCapabilities"]["sse"].as_bool().unwrap_or(false),
        session_methods,
        auth_logout: value["auth"]["logout"].is_object(),
    }
}

fn expected_capabilities(provider: ProviderId) -> StableCapabilities {
    match provider {
        ProviderId::Grok => StableCapabilities {
            load_session: true,
            image: false,
            audio: false,
            embedded_context: true,
            mcp_http: true,
            mcp_sse: true,
            session_methods: vec!["list"],
            auth_logout: false,
        },
        ProviderId::Cursor => StableCapabilities {
            load_session: true,
            image: true,
            audio: false,
            embedded_context: false,
            mcp_http: true,
            mcp_sse: true,
            session_methods: vec!["list"],
            auth_logout: false,
        },
        ProviderId::Codex => StableCapabilities {
            load_session: true,
            image: true,
            audio: false,
            embedded_context: true,
            mcp_http: true,
            mcp_sse: false,
            session_methods: vec!["additionalDirectories", "close", "delete", "list", "resume"],
            auth_logout: true,
        },
        ProviderId::Claude => StableCapabilities {
            load_session: true,
            image: true,
            audio: false,
            embedded_context: true,
            mcp_http: true,
            mcp_sse: true,
            session_methods: vec!["additionalDirectories", "close", "delete", "list", "resume"],
            auth_logout: true,
        },
    }
}
