#!/usr/bin/env python3
"""Check release versions by parsed package identity, not matching substrings."""
import argparse
import json
from pathlib import Path
import re
import tomllib


SWIFT_TOKENS_REL = "macos/Sources/AgentHubCore/DesignSystem/DesignTokens.swift"


def read_toml(root, name):
    return tomllib.loads((root / name).read_text(encoding="utf-8"))


def check(root, tag=None):
    workspace = read_toml(root, "Cargo.toml")["workspace"]
    expected = workspace["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", expected):
        raise ValueError("Cargo workspace version is not semantic versioning")
    if tag is not None and tag != f"v{expected}":
        raise ValueError("release tag does not match the Cargo workspace version")

    swift_tokens = (root / SWIFT_TOKENS_REL).read_text(encoding="utf-8")
    fallback_versions = re.findall(r'\bfallbackAppVersion(?:\s*:\s*String)?\s*=\s*"([^"]+)"', swift_tokens)
    if fallback_versions != [expected]:
        raise ValueError(f"{SWIFT_TOKENS_REL}: fallbackAppVersion mismatch")

    members = []
    for member in workspace["members"]:
        package = read_toml(root, f"{member}/Cargo.toml")["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError(f"{member}/Cargo.toml: must inherit the workspace version")
        members.append(package["name"])

    locked = read_toml(root, "Cargo.lock")["package"]
    for package in members:
        entries = [entry for entry in locked if entry["name"] == package]
        if len(entries) != 1 or entries[0]["version"] != expected or "source" in entries[0]:
            raise ValueError(f"Cargo.lock: local package {package} version mismatch")

    # Registry schema version describes its own format, not the app release.
    registry = json.loads((root / "contracts/agenthub-contract.json").read_text(encoding="utf-8"))
    schema = json.loads((root / "contracts/agenthub-contract.schema.json").read_text(encoding="utf-8"))
    if registry["schemaVersion"] != schema["properties"]["schemaVersion"]["const"]:
        raise ValueError("contract registry schema version mismatch")
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag")
    args = parser.parse_args()
    try:
        version = check(Path(__file__).resolve().parent.parent, args.tag)
    except (OSError, ValueError, KeyError, TypeError) as error:
        # Fixed repository paths and identifiers only; never dump manifest contents.
        raise SystemExit(f"version check failed: {type(error).__name__}") from None
    print(f"AgentHub {version} manifest and lock versions: ok")


if __name__ == "__main__":
    main()
