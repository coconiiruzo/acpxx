mod support;

#[cfg(unix)]
mod unix {
    use std::collections::BTreeMap;
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use acpxx::{
        AssertionResult, Broker, FailureCode, PermissionPolicy, ProbeObservation,
        ProcessDisposition, ProviderAssertions, ProviderId, ProviderSpec, TerminalRunState,
        WaitOptions,
    };
    use sha2::{Digest as _, Sha256};
    use uuid::Uuid;

    use super::support::{MockProviderFixture, mock_request};

    #[tokio::test]
    async fn future_and_nonsemver_versions_run_without_assertions() {
        let future = MockProviderFixture::new_with_marker(ProviderId::Grok, "future_version");
        let future_receipt = run(future.request("normal")).await;
        assert_eq!(future_receipt.state, TerminalRunState::Succeeded);
        let future_identity = future_receipt.provider_identity.as_ref().unwrap();
        assert_eq!(
            future_identity
                .observed_version
                .observed()
                .map(String::as_str),
            Some("9999.0.0"),
            "observation={:?}",
            future_identity.observed_version
        );

        let nonsemver = MockProviderFixture::new_with_marker(ProviderId::Cursor, "nonsemver");
        let nonsemver_receipt = run(nonsemver.request("normal")).await;
        assert_eq!(nonsemver_receipt.state, TerminalRunState::Succeeded);
        assert_eq!(
            nonsemver_receipt
                .provider_identity
                .as_ref()
                .and_then(|identity| identity.observed_version.observed())
                .map(String::as_str),
            Some("nightly-channel-future-build")
        );
    }

    #[tokio::test]
    async fn failed_and_timed_out_version_probes_are_observations_not_gates() {
        for marker in ["probe_fail", "probe_timeout"] {
            let fixture = MockProviderFixture::new_with_marker(ProviderId::Grok, marker);
            let receipt = run(fixture.request("normal")).await;
            assert_eq!(
                receipt.state,
                TerminalRunState::Succeeded,
                "marker={marker}"
            );
            assert!(matches!(
                receipt
                    .provider_identity
                    .as_ref()
                    .map(|identity| &identity.observed_version),
                Some(ProbeObservation::Unavailable(_))
            ));
        }

        let malformed = MockProviderFixture::new_with_marker(ProviderId::Grok, "malformed_version");
        let receipt = run(malformed.request("normal")).await;
        assert_eq!(receipt.state, TerminalRunState::Succeeded);
        assert!(matches!(
            receipt
                .provider_identity
                .as_ref()
                .map(|identity| &identity.observed_version),
            Some(ProbeObservation::Malformed(_))
        ));
    }

    #[tokio::test]
    async fn exact_version_and_digest_assertions_pass_or_fail_before_launch() {
        let fixture = MockProviderFixture::new_with_marker(ProviderId::Grok, "future_version");
        let digest = sha256(fixture.executable());
        let mut matching = fixture.request("normal");
        matching.assertions = ProviderAssertions {
            version: Some("9999.0.0".into()),
            components: BTreeMap::new(),
            launch_sha256: Some(digest),
        };
        let matched = run(matching).await;
        assert_eq!(matched.state, TerminalRunState::Succeeded);
        assert!(matches!(
            matched
                .provider_identity
                .as_ref()
                .map(|identity| &identity.assertion_result),
            Some(AssertionResult::Matched { .. })
        ));

        let mismatch = MockProviderFixture::new_with_marker(ProviderId::Grok, "future_version_bad");
        let mut request = mismatch.request("normal");
        request.assertions.version = Some("1.0.0".into());
        let failed = run(request).await;
        assert_eq!(failed.state, TerminalRunState::Failed);
        assert_eq!(
            failed.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderAssertionFailed)
        );
        assert_eq!(failed.cleanup.process, ProcessDisposition::NeverStarted);
        assert!(!mismatch.marker_path(".launched").exists());
        assert!(!serde_json::to_string(&failed).unwrap().contains("1.0.0"));

        let unavailable = MockProviderFixture::new_with_marker(ProviderId::Grok, "probe_fail");
        let mut request = unavailable.request("normal");
        request.assertions.version = Some("anything".into());
        let failed = run(request).await;
        assert_eq!(
            failed.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderAssertionFailed)
        );
        assert!(
            failed
                .failure
                .as_ref()
                .unwrap()
                .message
                .contains("could not be observed")
        );

        let digest_mismatch = MockProviderFixture::new(ProviderId::Grok);
        let mut request = digest_mismatch.request("normal");
        request.assertions.launch_sha256 = Some("00".repeat(32));
        let failed = run(request).await;
        assert_eq!(
            failed.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderAssertionFailed)
        );
        assert!(!digest_mismatch.marker_path(".launched").exists());
    }

    #[tokio::test]
    async fn component_assertions_are_exact_and_missing_metadata_is_best_effort() {
        let package = serde_json::json!({
            "name": "@agentclientprotocol/codex-acp",
            "dependencies": { "@openai/codex": "^999.4.2" }
        });
        let matching = MockProviderFixture::new_with_package_metadata(
            ProviderId::Codex,
            "component_match",
            package.clone(),
        );
        let mut request = matching.request("normal");
        request
            .assertions
            .components
            .insert("codex".into(), "999.4.2".into());
        assert_eq!(run(request).await.state, TerminalRunState::Succeeded);

        let mismatch = MockProviderFixture::new_with_package_metadata(
            ProviderId::Codex,
            "component_mismatch",
            package,
        );
        let mut request = mismatch.request("normal");
        request
            .assertions
            .components
            .insert("codex".into(), "1.0.0".into());
        let receipt = run(request).await;
        assert_eq!(
            receipt.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderAssertionFailed)
        );

        let missing = MockProviderFixture::new(ProviderId::Codex);
        let receipt = run(missing.request("normal")).await;
        assert_eq!(receipt.state, TerminalRunState::Succeeded);
        assert!(matches!(
            receipt
                .provider_identity
                .as_ref()
                .and_then(|identity| identity.observed_components.get("codex")),
            Some(ProbeObservation::Unavailable(_))
        ));

        let missing_asserted = MockProviderFixture::new(ProviderId::Codex);
        let mut request = missing_asserted.request("normal");
        request
            .assertions
            .components
            .insert("codex".into(), "1.2.3".into());
        let receipt = run(request).await;
        assert_eq!(
            receipt.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderAssertionFailed)
        );
        assert!(!missing_asserted.marker_path(".launched").exists());
    }

    #[tokio::test]
    async fn unsafe_or_nonregular_launch_artifacts_are_rejected() {
        let no_exec = MockProviderFixture::new_with_marker(ProviderId::Grok, "no_exec");
        std::fs::set_permissions(no_exec.executable(), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        assert_spawn_safety_failure(no_exec.request("normal")).await;

        let writable = MockProviderFixture::new_with_marker(ProviderId::Grok, "world_writable");
        std::fs::set_permissions(
            writable.executable(),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        assert_spawn_safety_failure(writable.request("normal")).await;

        let directory = std::env::temp_dir().join(format!("agentmux-directory-{}", Uuid::now_v7()));
        std::fs::create_dir(&directory).unwrap();
        let mut request = mock_request("normal", 0.0);
        request.provider = ProviderSpec::Grok {
            executable: Some(directory.clone()),
        };
        assert_spawn_safety_failure(request).await;
        std::fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn launch_artifact_replacement_during_probe_is_detected_before_spawn() {
        let fixture = MockProviderFixture::new_with_marker(ProviderId::Grok, "probe_gate");
        let broker = Broker::new(1);
        let spawned = broker.spawn(fixture.request("normal")).await.unwrap();
        wait_for_path(&fixture.marker_path(".probe-ready")).await;

        let replacement = fixture.marker_path(".replacement");
        std::fs::copy("/usr/bin/true", &replacement).unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&replacement, fixture.executable()).unwrap();
        std::fs::write(fixture.marker_path(".probe-continue"), b"continue").unwrap();

        let receipt = broker
            .wait_run(spawned.run, WaitOptions::default())
            .await
            .unwrap();
        assert_eq!(receipt.state, TerminalRunState::Failed);
        assert_eq!(
            receipt.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderArtifactChanged)
        );
        assert_eq!(receipt.cleanup.process, ProcessDisposition::NeverStarted);
        assert!(!fixture.marker_path(".launched").exists());
        broker.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn asserted_supplementary_metadata_is_rechecked() {
        let fixture = MockProviderFixture::new_with_package_metadata(
            ProviderId::Codex,
            "metadata_recheck",
            serde_json::json!({
                "name": "@agentclientprotocol/codex-acp",
                "dependencies": { "@openai/codex": "1.2.3" }
            }),
        );
        let driver = fixture.request("normal").provider.driver().unwrap();
        let observed = acpxx::acp::observe_provider(&driver).await.unwrap();
        let mut assertions = ProviderAssertions::default();
        assertions.components.insert("codex".into(), "1.2.3".into());
        let package = fixture
            .executable()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("package.json");
        std::fs::write(
            package,
            br#"{"name":"@agentclientprotocol/codex-acp","dependencies":{"@openai/codex":"9.9.9"}}"#,
        )
        .unwrap();
        assert!(observed.verify_unchanged(&assertions).is_err());
    }

    #[tokio::test]
    async fn acp_negotiation_completes_identity_and_rejects_unsupported_protocol() {
        let normal = MockProviderFixture::new_with_marker(ProviderId::Grok, "with_capability");
        let receipt = run(normal.request("normal")).await;
        let identity = receipt.provider_identity.unwrap();
        assert_eq!(identity.acp_protocol_version, Some(1));
        assert_eq!(
            identity
                .acp_agent_info
                .as_ref()
                .map(|info| info.name.as_str()),
            Some("mock-acp")
        );
        assert_eq!(
            identity.capability_digest.as_deref().map(str::len),
            Some(64)
        );

        let unsupported =
            MockProviderFixture::new_with_marker(ProviderId::Grok, "unsupported_protocol");
        let failed = run(unsupported.request("normal")).await;
        assert_eq!(failed.state, TerminalRunState::Failed);
        assert_eq!(
            failed.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::AcpInitializeFailed)
        );
    }

    #[tokio::test]
    async fn unknown_version_uses_the_same_permission_policy() {
        for (policy, mode) in [
            (PermissionPolicy::Deny, "permission"),
            (PermissionPolicy::AllowAll, "permission_allow"),
        ] {
            let fixture =
                MockProviderFixture::new_with_marker(ProviderId::Grok, "future_version_permission");
            let mut request = fixture.request(mode);
            request.permission_policy = policy;
            let receipt = run(request).await;
            assert_eq!(receipt.state, TerminalRunState::Succeeded);
            let identity = receipt.provider_identity.as_ref().unwrap();
            assert_eq!(
                identity.observed_version.observed().map(String::as_str),
                Some("9999.0.0"),
                "policy={policy:?} observation={:?}",
                identity.observed_version
            );
        }
    }

    #[tokio::test]
    async fn live_agent_keeps_identity_while_new_agent_observes_replaced_artifact() {
        let fixture = MockProviderFixture::new_with_marker(ProviderId::Grok, "identity_continuity");
        let broker = Broker::new(2);
        let first = broker.spawn(fixture.request("normal")).await.unwrap();
        let first_receipt = broker
            .wait_run(first.run, WaitOptions::default())
            .await
            .unwrap();
        let first_identity = first_receipt.provider_identity.clone().unwrap();

        replace_with_modified_fixture(fixture.executable());
        let followup = broker
            .followup(
                first.agent,
                first.run.run_id,
                acpxx::FollowupTask::new("continue"),
            )
            .await
            .unwrap();
        let followup_receipt = broker
            .wait_run(followup, WaitOptions::default())
            .await
            .unwrap();
        assert_eq!(
            followup_receipt.provider_identity,
            Some(first_identity.clone())
        );

        let second = broker.spawn(fixture.request("normal")).await.unwrap();
        let second_receipt = broker
            .wait_run(second.run, WaitOptions::default())
            .await
            .unwrap();
        assert_ne!(
            second_receipt.provider_identity.unwrap().launch_sha256,
            first_identity.launch_sha256
        );
        broker.shutdown().await.unwrap();
    }

    async fn run(request: acpxx::SpawnRequest) -> acpxx::RunReceipt {
        let broker = Broker::new(1);
        let spawned = broker.spawn(request).await.unwrap();
        let receipt = broker
            .wait_run(
                spawned.run,
                WaitOptions {
                    timeout: Some(Duration::from_secs(15)),
                },
            )
            .await
            .unwrap();
        broker.shutdown().await.unwrap();
        receipt
    }

    async fn assert_spawn_safety_failure(request: acpxx::SpawnRequest) {
        let receipt = run(request).await;
        assert_eq!(receipt.state, TerminalRunState::Failed);
        assert_eq!(
            receipt.failure.as_ref().map(|failure| failure.code),
            Some(FailureCode::ProviderSpawnFailed)
        );
        assert_eq!(receipt.cleanup.process, ProcessDisposition::NeverStarted);
    }

    fn sha256(path: &Path) -> String {
        format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
    }

    async fn wait_for_path(path: &Path) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !path.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {}", path.display()));
    }

    fn replace_with_modified_fixture(path: &Path) {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_acp_agent.py");
        let replacement = PathBuf::from(format!("{}.replacement", path.display()));
        std::fs::copy(source, &replacement).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&replacement)
            .unwrap();
        file.write_all(b"\n# replaced artifact\n").unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(replacement, path).unwrap();
    }
}
