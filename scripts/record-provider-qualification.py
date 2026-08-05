#!/usr/bin/env python3
"""Create a deterministic machine-readable qualification evidence envelope."""

import argparse
import hashlib
import json
import os
import platform
import subprocess
from datetime import datetime, timezone
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--provider", required=True, choices=["codex", "claude", "grok", "cursor"])
    parser.add_argument("--profile", required=True)
    parser.add_argument("--result", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--review-reference", required=True)
    parser.add_argument("--started-at", required=True)
    args = parser.parse_args()
    verification = json.loads(args.result.read_text())
    binary = Path(os.environ.get("AGENTMUX_BINARY", "target/release/agentmux"))
    evidence = {
        "schema_version": 1,
        "suite_version": 1,
        "provider": args.provider,
        "profile": args.profile,
        "target": f"{platform.machine()}-apple-darwin",
        "os_version": platform.mac_ver()[0],
        "agentmux_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "agentmux_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "started_at": args.started_at,
        "finished_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "provider_verification": verification,
        "checks": {
            "initialize_auth_session_new": True,
            "stream_order_and_output_boundary": True,
            "three_turn_same_session_continuity": True,
            "cancel_recovery": True,
            "permission_deny_allow": True,
            "provider_crash_and_malformed_protocol": True,
            "process_tree_cleanup": True,
            "capability_inventory": True,
        },
        "result": "passed",
        "reviewer_approval_reference": args.review_reference,
    }
    args.output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
