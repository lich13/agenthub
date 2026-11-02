#!/usr/bin/env python3
"""Check release versions by parsed package identity, not matching substrings."""
import argparse
import json
from pathlib import Path
import re
import tomllib


def read_toml(root, name):
    return tomllib.loads((root / name).read_text())


def check(root, tag=None):
    workspace = read_toml(root, "Cargo.toml")["workspace"]
    expected = workspace["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", expected):
        raise ValueError("Cargo workspace version is not semantic versioning")
    if tag is not None and tag != f"v{expected}":
        raise ValueError("release tag does not match the Cargo workspace version")
    for name in ["package.json", "webui/package.json", "src-tauri/tauri.conf.json"]:
        if json.loads((root / name).read_text())["version"] != expected:
            raise ValueError(f"{name}: version mismatch")
    members = []
    for member in workspace["members"]:
        package = read_toml(root, f"{member}/Cargo.toml")["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError(f"{member}/Cargo.toml: must inherit the workspace version")
        members.append(package["name"])
    desktop = read_toml(root, "src-tauri/Cargo.toml")["package"]
    if desktop["version"] != expected:
        raise ValueError("src-tauri/Cargo.toml: version mismatch")
    for name, packages in [
        ("Cargo.lock", members),
        ("src-tauri/Cargo.lock", [desktop["name"], "agenthub-core"]),
    ]:
        locked = read_toml(root, name)["package"]
        for package in packages:
            entries = [entry for entry in locked if entry["name"] == package]
            if len(entries) != 1 or entries[0]["version"] != expected or "source" in entries[0]:
                raise ValueError(f"{name}: local package {package} version mismatch")
    # Registry schema version describes its own format, not the app release.
    registry = json.loads((root / "contracts/agenthub-contract.json").read_text())
    schema = json.loads((root / "contracts/agenthub-contract.schema.json").read_text())
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
