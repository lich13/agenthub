#!/usr/bin/env python3
"""Check unified Cargo workspace inheritance, direct lock consistency, and Swift 6 package alignment."""
from pathlib import Path
import re
import tomllib


def load(root, name):
    return tomllib.loads((root / name).read_text(encoding="utf-8"))


def direct_version(packages, owner, dependency):
    package, = [p for p in packages if p["name"] == owner and "source" not in p]
    edge, = [edge.split() for edge in package.get("dependencies", []) if edge.split()[0] == dependency]
    candidates = [p for p in packages if p["name"] == dependency and (len(edge) == 1 or p["version"] == edge[1])]
    resolved, = candidates
    return resolved["version"]


def check(root):
    if (root / "src-tauri/Cargo.toml").exists() or (root / "webui/package.json").exists():
        raise ValueError("retired Tauri or WebUI workspace must not exist")

    workspace = load(root, "Cargo.toml")["workspace"]
    workspace_pkg = workspace["package"]
    if not workspace_pkg.get("rust-version") or not workspace_pkg.get("edition"):
        raise ValueError("Cargo workspace must declare rust-version and edition")

    members = workspace["members"]
    if sorted(members) != ["crates/agenthub-core", "crates/agenthub-ffi", "crates/agenthub-webd"]:
        raise ValueError("unexpected Cargo workspace member set")

    lock_packages = load(root, "Cargo.lock")["package"]
    member_names = []
    deps_by_member = {}
    for member in members:
        manifest = load(root, f"{member}/Cargo.toml")
        pkg = manifest["package"]
        for inherited_key in ("version", "edition", "rust-version"):
            if pkg.get(inherited_key) != {"workspace": True}:
                raise ValueError(f"{member}/Cargo.toml must inherit workspace {inherited_key}")
        name = pkg["name"]
        member_names.append(name)
        deps_by_member[name] = set(manifest.get("dependencies", {}).keys()) - set(member_names)

    all_external = set().union(*deps_by_member.values())
    shared_count = 0
    for dep in sorted(all_external):
        owners = [owner for owner in member_names if dep in deps_by_member[owner] and dep not in member_names]
        if len(owners) < 2:
            continue
        shared_count += 1
        versions = {direct_version(lock_packages, owner, dep) for owner in owners}
        if len(versions) != 1:
            raise ValueError(f"workspace members resolve different direct versions for {dep}")

    swift_pkg = (root / "macos/Package.swift").read_text(encoding="utf-8")
    if not re.search(r"^//\s*swift-tools-version:\s*6\.0\b", swift_pkg, re.MULTILINE):
        raise ValueError("macos/Package.swift must declare swift-tools-version: 6.0")
    if ".macOS(.v12)" not in swift_pkg:
        raise ValueError("macos/Package.swift must target macOS 12")

    return shared_count


if __name__ == "__main__":
    try:
        count = check(Path(__file__).resolve().parent.parent)
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"workspace check failed: {type(error).__name__}") from None
    print(f"Unified Cargo + Swift 6 workspace and {count} shared dependency resolutions: ok")
