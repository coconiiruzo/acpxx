#!/usr/bin/env python3
"""Render the public compatibility matrix from an exact Catalog JSON file."""

import argparse
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("catalog", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    catalog = json.loads(args.catalog.read_text())
    if catalog.get("schema_version") != 1:
        raise SystemExit("only Catalog schema v1 can be rendered")
    providers = {"codex", "claude", "grok", "cursor"}
    seen = {entry["provider"] for entry in catalog["entries"]}
    if seen != providers:
        raise SystemExit(f"Catalog provider set is not closed: {sorted(seen)}")
    lines = [
        "# Provider Compatibility",
        "",
        "> Generated from `compatibility/bootstrap/catalog-v1.json`; do not edit the table by hand.",
        "",
        f"Catalog `{catalog['catalog_id']}` sequence `{catalog['sequence']}` was generated "
        f"at `{catalog['generated_at']}` and expires at `{catalog['expires_at']}`.",
        "",
        "| Provider | Identity | Components | Driver | Target | State | Evidence |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for entry in sorted(catalog["entries"], key=lambda item: item["provider"]):
        components = ", ".join(
            f"{name} {value}" for name, value in sorted(entry["identity"]["components"].items())
        ) or "—"
        lines.append(
            f"| {entry['provider'].title()} | `{entry['identity']['display_version']}` | "
            f"{components} | `{entry['driver_id']}` r{entry['driver_revision']} | "
            f"`{entry['target']}` | **{entry['state']}** | "
            f"`{entry['qualification']['evidence_digest']}` |"
        )
    lines.extend(
        [
            "",
            "Runtime authorization requires an exact identity and artifact-set match against the signed Catalog.",
            "A version string alone is never sufficient. Candidate discovery does not publish or authorize a version;",
            "only controlled qualification plus an approved, higher-sequence signed Catalog update can do so.",
            "",
            "Use `agentmux compatibility status` and `agentmux provider verify PROFILE` to inspect the active",
            "Catalog and an installed artifact without starting a provider session.",
            "",
        ]
    )
    args.output.write_text("\n".join(lines))


if __name__ == "__main__":
    main()
