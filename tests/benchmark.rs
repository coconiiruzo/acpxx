#[cfg(unix)]
#[test]
fn benchmark_cli_reports_integrity_without_enforcing_debug_budgets() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agentmux"))
        .args([
            "benchmark",
            "--samples",
            "10",
            "--events",
            "10000",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["samples"], 10);
    assert_eq!(report["requested_events"], 10_000);
    assert_eq!(report["measurements"]["synthetic_events_delivered"], 10_000);
    assert_eq!(report["host"]["release_build"], false);
    assert_eq!(report["qualification_eligible"], false);
    assert!(report["passed"].is_null());
}
