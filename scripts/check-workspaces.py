#!/usr/bin/env python3
"""Check shared declarations and direct dependency resolutions of both workspaces."""
from pathlib import Path
import re
import tomllib

# Existing intentional lock difference. Upgrades review this exact pair rather
# than forcing Tauri's unrelated transitive major versions to match the server.
KNOWN_LOCK_DIFFERENCES = {"toml": ("0.8.23", "0.8.2")}


def load(root, name):
    return tomllib.loads((root / name).read_text())


def requirement(value):
    return value if isinstance(value, str) else value["version"]


def direct_version(packages, owner, dependency):
    package, = [p for p in packages if p["name"] == owner and "source" not in p]
    edge, = [edge.split() for edge in package["dependencies"] if edge.split()[0] == dependency]
    candidates = [p for p in packages if p["name"] == dependency and (len(edge) == 1 or p["version"] == edge[1])]
    resolved, = candidates
    return resolved["version"]


def check(root):
    workspace = load(root, "Cargo.toml")["workspace"]
    desktop = load(root, "src-tauri/Cargo.toml")
    toolchain = load(root, "rust-toolchain.toml")["toolchain"]
    pinned = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", toolchain["channel"])
    if pinned is None:
        raise ValueError("expected an exact stable Rust toolchain")
    minimum = ".".join(pinned.groups()[:2])
    if workspace["package"]["rust-version"] != minimum:
        raise ValueError("minimum Rust must match the pinned development toolchain")
    if desktop["package"]["rust-version"] != workspace["package"]["rust-version"]:
        raise ValueError("Tauri and core must declare the same MSRV")
    core_lock = load(root, "Cargo.lock")["package"]
    desktop_lock = load(root, "src-tauri/Cargo.lock")["package"]
    core_deps = load(root, "crates/agenthub-core/Cargo.toml")["dependencies"]
    shared = workspace["dependencies"].keys() & desktop["dependencies"].keys()
    observed = {}
    for name in sorted(shared):
        if requirement(workspace["dependencies"][name]) != requirement(desktop["dependencies"][name]):
            raise ValueError(f"shared dependency requirement differs: {name}")
        versions = (direct_version(core_lock, "agenthub-core" if name in core_deps else "agenthub-webd", name), direct_version(desktop_lock, "agenthub-desktop", name))
        # The core crate is also compiled in Tauri's independent graph.
        if name in core_deps and direct_version(desktop_lock, "agenthub-core", name) != versions[1]:
            raise ValueError(f"desktop and embedded core resolve different direct versions: {name}")
        if versions[0] != versions[1]:
            observed[name] = versions
    if observed != KNOWN_LOCK_DIFFERENCES:
        raise ValueError("shared lock resolutions changed; review the explicit compatibility baseline")
    return len(shared)


if __name__ == "__main__":
    try:
        count = check(Path(__file__).resolve().parent.parent)
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"workspace check failed: {type(error).__name__}") from None
    print(f"Dual workspace MSRV and {count} shared dependency resolutions: ok")
