"""Regression tests for workspace MSRV and shared dependency resolution checks."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


# Dynamic source imports must not persist interpreter paths in repository bytecode.
sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("check_workspaces", Path(__file__).with_name("check-workspaces.py"))
check_workspaces = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_workspaces)


SHARED_VERSIONS = {
    "anyhow": "1.0.102",
    "chrono": "0.4.45",
    "dirs": "5.0.1",
    "reqwest": "0.12.28",
    "serde": "1.0.228",
    "serde_json": "1.0.150",
    "tokio": "1.52.3",
    "toml": "0.8.23",
}
DESKTOP_VERSIONS = dict(SHARED_VERSIONS)


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


def local_dependency_entries(versions):
    return [f"{name} {version}" for name, version in sorted(versions.items())]


def make_fixture(root, desktop_versions=None, extra_desktop_packages=()):
    write(root / "rust-toolchain.toml", '[toolchain]\nchannel = "1.99.0"\n')
    desktop_versions = desktop_versions or DESKTOP_VERSIONS
    names = sorted(SHARED_VERSIONS)
    requirements = {
        "anyhow": '"1.0"',
        "chrono": '"0.4"',
        "dirs": '"5.0"',
        "reqwest": '"0.12"',
        "serde": '"1.0"',
        "serde_json": '"1.0"',
        "tokio": '"1.37"',
        "toml": '"0.8"',
    }
    workspace_deps = "\n".join(f"{name} = {requirements[name]}" for name in names)
    desktop_deps = "\n".join(
        f"{name} = {{ version = {requirements[name]}, default-features = false }}" if name == "reqwest"
        else f"{name} = {{ version = {requirements[name]} }}" if name == "serde"
        else f"{name} = {requirements[name]}"
        for name in names
    )
    write(root / "Cargo.toml", f'''\
[workspace]
members = ["crates/agenthub-core", "crates/agenthub-webd"]

[workspace.package]
rust-version = "1.99"

[workspace.dependencies]
{workspace_deps}
''')
    write(root / "src-tauri/Cargo.toml", f'''\
[package]
name = "agenthub-desktop"
rust-version = "1.99"

[dependencies]
{desktop_deps}
agenthub-core = {{ path = "../crates/agenthub-core" }}
''')
    core_names = [name for name in names if name != "reqwest"]
    core_deps = "\n".join(f"{name} = {{ workspace = true }}" for name in core_names)
    write(root / "crates/agenthub-core/Cargo.toml", f'''\
[package]
name = "agenthub-core"

[dependencies]
{core_deps}
''')
    write(root / "crates/agenthub-webd/Cargo.toml", '''\
[package]
name = "agenthub-webd"

[dependencies]
reqwest = { workspace = true }
''')

    core_entries = [
        package_entry("agenthub-core", "1.2.21", local_dependency_entries({name: SHARED_VERSIONS[name] for name in core_names}), local=True),
        package_entry("agenthub-webd", "1.2.21", [f'reqwest {SHARED_VERSIONS["reqwest"]}'], local=True),
    ]
    core_entries.extend(package_entry(name, version) for name, version in SHARED_VERSIONS.items())
    write(root / "Cargo.lock", "version = 4\n\n" + "\n\n".join(core_entries) + "\n")

    desktop_entries = [
        package_entry("agenthub-core", "1.2.21", local_dependency_entries({name: desktop_versions[name] for name in core_names}), local=True),
        package_entry("agenthub-desktop", "1.2.21", local_dependency_entries(desktop_versions), local=True),
    ]
    desktop_entries.extend(package_entry(name, version) for name, version in desktop_versions.items())
    desktop_entries.extend(package_entry(name, version) for name, version in extra_desktop_packages)
    write(root / "src-tauri/Cargo.lock", "version = 4\n\n" + "\n\n".join(desktop_entries) + "\n")


class WorkspaceCheckTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="agenthub-workspace-check-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        make_fixture(self.root)

    def test_matching_shared_dependency_edges_pass(self):
        self.assertEqual(check_workspaces.check(self.root), 8)

    def test_unrelated_tauri_lock_versions_do_not_change_direct_resolutions(self):
        make_fixture(self.root, extra_desktop_packages=[
            ("dirs", "6.0.0"),
            ("reqwest", "0.13.4"),
            ("toml", "0.9.12"),
            ("toml", "1.1.2+spec-1.1.0"),
        ])
        self.assertEqual(check_workspaces.check(self.root), 8)

    def test_msrv_must_match_between_workspace_and_tauri(self):
        manifest = self.root / "src-tauri/Cargo.toml"
        manifest.write_text(manifest.read_text(encoding="utf-8").replace('rust-version = "1.99"', 'rust-version = "1.98"'), encoding="utf-8")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_matching_older_minimum_does_not_restore_another_toolchain(self):
        for relative in ("Cargo.toml", "src-tauri/Cargo.toml"):
            manifest = self.root / relative
            manifest.write_text(manifest.read_text(encoding="utf-8").replace('rust-version = "1.99"', 'rust-version = "1.98"'), encoding="utf-8")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_unpinned_rust_channel_is_rejected(self):
        toolchain = self.root / "rust-toolchain.toml"
        toolchain.write_text('[toolchain]\nchannel = "stable"\n', encoding="utf-8")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_shared_dependency_requirements_must_match(self):
        manifest = self.root / "src-tauri/Cargo.toml"
        manifest.write_text(manifest.read_text(encoding="utf-8").replace('anyhow = "1.0"', 'anyhow = "1.1"'), encoding="utf-8")
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)

    def test_unapproved_direct_lock_resolution_difference_is_rejected(self):
        mismatched = {**DESKTOP_VERSIONS, "anyhow": "1.0.103"}
        make_fixture(self.root, desktop_versions=mismatched)
        with self.assertRaises(ValueError):
            check_workspaces.check(self.root)


if __name__ == "__main__":
    unittest.main()
