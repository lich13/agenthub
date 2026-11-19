"""Regression tests for unified Cargo + Swift 6 workspace checks."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("check_workspaces", Path(__file__).with_name("check-workspaces.py"))
check_workspaces = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_workspaces)


SHARED_VERSIONS = {
    "anyhow": "1.0.102",
    "chrono": "0.4.45",
    "dirs": "5.0.1",
    "reqwest": "0.13.4",
    "serde": "1.0.228",
    "serde_json": "1.0.150",
    "tokio": "1.52.3",
    "toml": "0.8.23",
}


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def package_entry(name, version, dependencies=(), local=False):
    lines = ["[[package]]", f'name = "{name}"', f'version = "{version}"']
    if not local:
        lines.append('source = "registry+https://example.invalid/index"')
    if dependencies:
        lines.append("dependencies = [" + ", ".join(f'"{item}"' for item in dependencies) + "]")
    return "\n".join(lines)


def make_fixture(root, ffi_anyhow_version=None):
    ffi_anyhow = ffi_anyhow_version or SHARED_VERSIONS["anyhow"]
    write(root / "Cargo.toml", '''\
[workspace]
members = ["crates/agenthub-core", "crates/agenthub-ffi", "crates/agenthub-webd"]

[workspace.package]
version = "2.1.28"
edition = "2021"
rust-version = "1.99"
''')
    for rel, name in (
        ("crates/agenthub-core/Cargo.toml", "agenthub-core"),
        ("crates/agenthub-ffi/Cargo.toml", "agenthub-ffi"),
        ("crates/agenthub-webd/Cargo.toml", "agenthub-webd"),
    ):
        deps = "\n".join(f"{dep} = {{ workspace = true }}" for dep in sorted(SHARED_VERSIONS))
        if name != "agenthub-core":
            deps += '\nagenthub-core = { path = "../agenthub-core" }'
        write(root / rel, f'''\
[package]
name = "{name}"
version = {{ workspace = true }}
edition = {{ workspace = true }}
rust-version = {{ workspace = true }}

[dependencies]
{deps}
''')
    write(root / "macos/Package.swift", '''\
// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "AgentHub",
    platforms: [.macOS(.v12)]
)
''')

    core_edges = [f"{k} {v}" for k, v in sorted(SHARED_VERSIONS.items())]
    ffi_map = dict(SHARED_VERSIONS, anyhow=ffi_anyhow)
    ffi_edges = ["agenthub-core 2.1.28", *(f"{k} {v}" for k, v in sorted(ffi_map.items()))]
    webd_edges = ["agenthub-core 2.1.28", *core_edges]

    entries = [
        package_entry("agenthub-core", "2.1.28", core_edges, local=True),
        package_entry("agenthub-ffi", "2.1.28", ffi_edges, local=True),
        package_entry("agenthub-webd", "2.1.28", webd_edges, local=True),
    ]
    entries.extend(package_entry(k, v) for k, v in sorted(SHARED_VERSIONS.items()))
    if ffi_anyhow != SHARED_VERSIONS["anyhow"]:
        entries.append(package_entry("anyhow", ffi_anyhow))
    write(root / "Cargo.lock", "version = 4\n\n" + "\n\n".join(entries) + "\n")


class WorkspaceCheckTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="agenthub-workspace-check-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        make_fixture(self.root)

    def test_unified_workspace_and_swift6_manifest_pass(self):
        self.assertEqual(check_workspaces.check(self.root), 8)

    def test_retired_tauri_or_webui_directory_fails(self):
        write(self.root / "src-tauri/Cargo.toml", "[package]\nname = \"retired\"\n")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_member_must_inherit_workspace_msrv(self):
        manifest = self.root / "crates/agenthub-ffi/Cargo.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                "rust-version = { workspace = true }",
                'rust-version = "1.98"',
            ),
            encoding="utf-8",
        )
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_divergent_shared_dependency_resolution_is_rejected(self):
        make_fixture(self.root, ffi_anyhow_version="1.0.103")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_swift_package_tools_version_and_macos12_target_are_enforced(self):
        swift_pkg = self.root / "macos/Package.swift"
        swift_pkg.write_text("// swift-tools-version: 5.9\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)


if __name__ == "__main__":
    unittest.main()
