#!/usr/bin/env python3
"""Render the release-specific Homebrew formula from its checked-in template."""

import argparse
import pathlib


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    template = (root / "packaging/homebrew/agentmux.rb.in").read_text(encoding="utf-8")
    rendered = template.replace("@VERSION@", args.version).replace(
        "@SHA256@", args.sha256
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
