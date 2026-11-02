"""Regression tests for release version and package identity checks."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


# Dynamic source imports must not persist interpreter paths in repository bytecode.
sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("check_versions", Path(__file__).with_name("check-versions.py"))
check_versions = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_versions)


VERSION = "1.2.21"


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def lock_package(name, version, source=False):
    lines = ["[[package]]", f'name = "{name}"', f'version = "{version}"']
    if source:
        lines.append('source = "registry+https://example.invalid/index"')
    return "\n".join(lines)


def make_fixture(root):
    write(root / "Cargo.toml", '''\
[workspace]
members = ["crates/agenthub-core", "crates/agenthub-webd"]

[workspace.package]
version = "1.2.21"
''')
    for path, name in (
        ("crates/agenthub-core/Cargo.toml", "agenthub-core"),
        ("crates/agenthub-webd/Cargo.toml", "agenthub-webd"),
    ):
        write(root / path, f'''\
[package]
name = "{name}"
version = {{ workspace = true }}
''')
    write(root / "src-tauri/Cargo.toml", '''\
[package]
name = "agenthub-desktop"
version = "1.2.21"
''')
    for path in ("package.json", "webui/package.json", "src-tauri/tauri.conf.json"):
        write(root / path, json.dumps({"version": VERSION}))
    write(root / "Cargo.lock", "\n\n".join([
        "version = 4",
        lock_package("agenthub-core", VERSION),
        lock_package("agenthub-webd", VERSION),
    ]) + "\n")
    write(root / "src-tauri/Cargo.lock", "\n\n".join([
        "version = 4",
        lock_package("agenthub-core", VERSION),
        lock_package("agenthub-desktop", VERSION),
    ]) + "\n")
    write(root / "contracts/agenthub-contract.json", json.dumps({"schemaVersion": 4}))
    write(root / "contracts/agenthub-contract.schema.json", json.dumps({
        "properties": {"schemaVersion": {"const": 4}},
    }))


class VersionCheckTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="agenthub-version-check-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        make_fixture(self.root)

    def test_correct_manifests_and_matching_tag_pass(self):
        self.assertEqual(check_versions.check(self.root, f"v{VERSION}"), VERSION)

    def test_release_manifests_must_match_workspace_version(self):
        for path in ("package.json", "webui/package.json", "src-tauri/tauri.conf.json"):
            with self.subTest(path=path):
                manifest = self.root / path
                manifest.write_text(json.dumps({"version": "9.9.9"}), encoding="utf-8")
                with self.assertRaises(ValueError):
                    check_versions.check(self.root)
                manifest.write_text(json.dumps({"version": VERSION}), encoding="utf-8")

    def test_release_tag_must_match_exact_version(self):
        with self.assertRaises(ValueError):
            check_versions.check(self.root, "v1.2.20")

    def test_wrong_local_lock_package_is_not_masked_by_unrelated_correct_version(self):
        lock = self.root / "Cargo.lock"
        contents = lock.read_text(encoding="utf-8").replace(
            'name = "agenthub-core"\nversion = "1.2.21"',
            'name = "agenthub-core"\nversion = "9.9.9"',
        )
        lock.write_text(contents + "\n" + lock_package("unrelated-package", VERSION, source=True) + "\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            check_versions.check(self.root)

    def test_duplicate_local_lock_identity_is_rejected(self):
        lock = self.root / "Cargo.lock"
        lock.write_text(
            lock.read_text(encoding="utf-8") + "\n" + lock_package("agenthub-core", VERSION) + "\n",
            encoding="utf-8",
        )
        with self.assertRaises(ValueError):
            check_versions.check(self.root)

    def test_registry_source_on_local_lock_package_is_rejected(self):
        lock = self.root / "src-tauri/Cargo.lock"
        contents = lock.read_text(encoding="utf-8").replace(
            'name = "agenthub-desktop"\nversion = "1.2.21"',
            'name = "agenthub-desktop"\nversion = "1.2.21"\nsource = "registry+https://example.invalid/index"',
        )
        lock.write_text(contents, encoding="utf-8")
        with self.assertRaises(ValueError):
            check_versions.check(self.root)

    def test_workspace_member_must_inherit_version(self):
        member = self.root / "crates/agenthub-webd/Cargo.toml"
        member.write_text(member.read_text(encoding="utf-8").replace(
            "version = { workspace = true }", f'version = "{VERSION}"',
        ), encoding="utf-8")
        with self.assertRaises(ValueError):
            check_versions.check(self.root)

    def test_contract_registry_schema_version_must_match_schema(self):
        registry = self.root / "contracts/agenthub-contract.json"
        registry.write_text(json.dumps({"schemaVersion": 3}), encoding="utf-8")
        with self.assertRaises(ValueError):
            check_versions.check(self.root)


if __name__ == "__main__":
    unittest.main()
