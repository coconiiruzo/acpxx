#!/usr/bin/env python3
"""Generate deterministic SPDX 2.3 SBOM and third-party license inventory."""

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import subprocess
import tomllib


def spdx_id(package_id: str) -> str:
    digest = hashlib.sha256(package_id.encode()).hexdigest()[:16]
    return f"SPDXRef-Package-{digest}"


def declared_license(value: str | None) -> str:
    if not value:
        return "NOASSERTION"
    # A few older crates still publish Cargo's pre-SPDX slash spelling.
    return value.replace("MIT/Apache-2.0", "MIT OR Apache-2.0")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sbom", required=True, type=pathlib.Path)
    parser.add_argument("--licenses", required=True, type=pathlib.Path)
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parent.parent
    metadata = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--filter-platform",
                "aarch64-apple-darwin",
            ],
            cwd=root,
            text=True,
        )
    )
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    checksums = {
        (package["name"], package["version"], package.get("source")): package.get(
            "checksum"
        )
        for package in lock["package"]
    }
    root_id = metadata["resolve"]["root"]
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    reachable = {root_id}
    pending = [root_id]
    while pending:
        node_id = pending.pop()
        for dependency in nodes[node_id]["deps"]:
            if dependency["dep_kinds"] and all(
                kind["kind"] == "dev" for kind in dependency["dep_kinds"]
            ):
                continue
            dependency_id = dependency["pkg"]
            if dependency_id not in reachable:
                reachable.add(dependency_id)
                pending.append(dependency_id)
    packages = sorted(
        (package for package in metadata["packages"] if package["id"] in reachable),
        key=lambda item: item["id"],
    )
    ids = {package["id"]: spdx_id(package["id"]) for package in packages}
    root_package = next(package for package in packages if package["id"] == root_id)
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    created = datetime.datetime.fromtimestamp(epoch, datetime.UTC).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    revision = os.environ.get("GITHUB_SHA", "source")

    spdx_packages = []
    for package in packages:
        checksum = checksums.get(
            (package["name"], package["version"], package.get("source"))
        )
        entry = {
            "SPDXID": ids[package["id"]],
            "name": package["name"],
            "versionInfo": package["version"],
            "downloadLocation": package.get("repository") or "NOASSERTION",
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": declared_license(package.get("license")),
            "copyrightText": "NOASSERTION",
        }
        if checksum:
            entry["checksums"] = [
                {"algorithm": "SHA256", "checksumValue": checksum}
            ]
        spdx_packages.append(entry)

    relationships = [
        {
            "spdxElementId": "SPDXRef-DOCUMENT",
            "relationshipType": "DESCRIBES",
            "relatedSpdxElement": ids[root_id],
        }
    ]
    for node_id in sorted(reachable):
        for dependency in sorted(nodes[node_id]["deps"], key=lambda item: item["pkg"]):
            if dependency["pkg"] not in reachable:
                continue
            if dependency["dep_kinds"] and all(
                kind["kind"] == "dev" for kind in dependency["dep_kinds"]
            ):
                continue
            relationships.append(
                {
                    "spdxElementId": ids[node_id],
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": ids[dependency["pkg"]],
                }
            )

    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"agentmux-{root_package['version']}",
        "documentNamespace": (
            "https://github.com/coconiiruzo/acpxx/sbom/"
            f"agentmux-{root_package['version']}-{revision}"
        ),
        "creationInfo": {
            "created": created,
            "creators": ["Tool: agentmux-generate-supply-chain"],
        },
        "packages": spdx_packages,
        "relationships": relationships,
    }
    args.sbom.parent.mkdir(parents=True, exist_ok=True)
    args.sbom.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    dependencies = [package for package in packages if package["id"] != root_id]
    lines = [
        "# Third-party licenses",
        "",
        "Generated from the locked Cargo dependency graph. License identifiers are",
        "reported from package metadata; consult each package source for license text.",
        "",
        "| Package | Version | Declared license | Source |",
        "| --- | --- | --- | --- |",
    ]
    for package in dependencies:
        source = package.get("repository") or package.get("source") or "local"
        license_id = declared_license(package.get("license"))
        lines.append(
            f"| {package['name']} | {package['version']} | {license_id} | {source} |"
        )
    args.licenses.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
