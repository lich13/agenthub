#!/usr/bin/env python3
"""Exercise installer archive and rollback behavior with isolated fixtures."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shlex
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SAFETY_PATH = ROOT / "deploy/agenthub-webd/install-safety.py"
INSTALL_PATH = ROOT / "deploy/agenthub-webd/install.sh"
UPDATE_PATH = ROOT / "deploy/agenthub-webd/update.sh"
UPDATER_PUB_PATH = ROOT / "deploy/agenthub-webd/updater.pub"
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("install_safety", SAFETY_PATH)
if spec is None or spec.loader is None:
    raise RuntimeError("could not load installer safety module")
safety = importlib.util.module_from_spec(spec)
spec.loader.exec_module(safety)


def add_member(archive: tarfile.TarFile, name: str, kind: bytes, data: bytes = b"", mode: int = 0o644, linkname: str = "") -> None:
    member = tarfile.TarInfo(name)
    member.type = kind
    member.mode = mode
    member.linkname = linkname
    if kind in (tarfile.REGTYPE, tarfile.AREGTYPE):
        member.size = len(data)
        archive.addfile(member, io.BytesIO(data))
    else:
        archive.addfile(member)


def make_archive(path: Path, members: list[tuple[str, bytes, bytes, int, str]]) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, kind, data, mode, linkname in members:
            add_member(archive, name, kind, data, mode, linkname)


def elf64_header(machine: int = 62) -> bytes:
    ident = b"\x7fELF" + bytes((2, 1, 1, 0, 0)) + bytes(7)
    fields = struct.pack("<HHIQQQIHHHHHH", 2, machine, 1, 0x400000, 64, 0, 0, 64, 56, 1, 64, 0, 0)
    return ident + fields


class InstallSafetyTests(unittest.TestCase):
    PAYLOAD_NAME = "agenthub-webd-linux-x86_64"
    REQUIRED_PAYLOAD_FILES = (
        "bin/agenthub-webd",
        "deploy/install.sh",
        "deploy/update.sh",
        "deploy/install-safety.py",
        "deploy/updater.pub",
    )

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="agenthub-install-test-")
        self.root = Path(self.temp.name)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def assert_archive_rejected(self, path: Path) -> None:
        destination = self.root / f"extract-{path.name}"
        destination.mkdir(parents=True)
        with self.assertRaises((ValueError, OSError, tarfile.TarError, EOFError)):
            safety.extract(path, destination)
        self.assertEqual(list(destination.iterdir()), [])

    def make_payload(
        self,
        parent: Path | None = None,
        *,
        header: bytes | None = None,
        missing: tuple[str, ...] = (),
    ) -> Path:
        parent = self.root if parent is None else parent
        parent.mkdir(parents=True, exist_ok=True)
        root = parent / self.PAYLOAD_NAME
        root.mkdir()
        for relative in self.REQUIRED_PAYLOAD_FILES:
            if relative in missing:
                continue
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(header if relative == "bin/agenthub-webd" and header is not None else b"fixture\n")
        return root

    def assert_payload_rejected_before_binary(
        self,
        root: Path,
        expected_error: str,
        signed_version: str = "1.2.23",
    ) -> None:
        with mock.patch.object(safety.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, expected_error):
                safety.validate_payload(root, signed_version)
            run.assert_not_called()

    def test_validate_payload_accepts_elf64_le_x86_64_and_signed_version(self) -> None:
        root = self.make_payload(header=elf64_header())
        binary = root / "bin/agenthub-webd"
        result = mock.Mock(returncode=0, stdout=b"agenthub-webd 1.2.23\n")

        with mock.patch.object(safety.subprocess, "run", return_value=result) as run:
            safety.validate_payload(root, "1.2.23")

        run.assert_called_once_with([str(binary), "--version"], capture_output=True, timeout=30, check=False)

    def test_validate_payload_rejects_invalid_payload_before_binary(self) -> None:
        cases = (
            ("missing required file", {"missing": ("deploy/updater.pub",)}, "invalid release layout"),
            ("extra root", {"extra_root": True}, "invalid release layout"),
            ("retired webui layout", {"retired": "webui"}, "retired release layout"),
            ("retired web-update layout", {"retired": "deploy/web-update.sh"}, "retired release layout"),
            ("wrong platform", {"header": elf64_header(183)}, "invalid release platform"),
        )
        for name, options, expected_error in cases:
            with self.subTest(case=name):
                parent = self.root / name.replace(" ", "-")
                root = self.make_payload(
                    parent,
                    header=options.get("header", elf64_header()),
                    missing=options.get("missing", ()),
                )
                if options.get("extra_root"):
                    (parent / "extra-root").mkdir()
                retired = options.get("retired")
                if retired == "webui":
                    (root / "webui").mkdir()
                elif retired is not None:
                    retired_path = root / retired
                    retired_path.parent.mkdir(parents=True, exist_ok=True)
                    retired_path.write_bytes(b"retired fixture\n")
                self.assert_payload_rejected_before_binary(
                    root,
                    expected_error,
                    options.get("signed_version", "1.2.23"),
                )

    def test_validate_payload_rejects_binary_version_mismatch(self) -> None:
        root = self.make_payload(header=elf64_header())
        binary = root / "bin/agenthub-webd"
        result = mock.Mock(returncode=0, stdout=b"agenthub-webd 1.2.24\n")

        with mock.patch.object(safety.subprocess, "run", return_value=result) as run:
            with self.assertRaisesRegex(ValueError, "release binary version mismatch"):
                safety.validate_payload(root, "1.2.23")

        run.assert_called_once_with([str(binary), "--version"], capture_output=True, timeout=30, check=False)

    def test_checksum_rejects_bad_digest_wrong_target_and_multiple_lines(self) -> None:
        archive = self.root / "release.tar.gz"
        archive.write_bytes(b"synthetic archive bytes")
        good = hashlib.sha256(archive.read_bytes()).hexdigest()
        checksum = self.root / "release.sha256"

        checksum.write_text(f"{'0' * 64}  {archive.name}\n", encoding="ascii")
        with self.assertRaises(ValueError):
            safety.verify_checksum(archive, checksum)

        checksum.write_text(f"{good}  other.tar.gz\n", encoding="ascii")
        with self.assertRaises(ValueError):
            safety.verify_checksum(archive, checksum)

        checksum.write_text(f"{good}  {archive.name}\n{good}  {archive.name}\n", encoding="ascii")
        with self.assertRaises(ValueError):
            safety.verify_checksum(archive, checksum)

    def test_extract_rejects_corrupt_gzip(self) -> None:
        archive = self.root / "broken.tar.gz"
        archive.write_bytes(b"not a gzip archive")
        self.assert_archive_rejected(archive)

    def test_extract_rejects_unsafe_paths_and_archive_member_types(self) -> None:
        bad_members = {
            "parent traversal": [("../escape", tarfile.REGTYPE, b"x", 0o644, "")],
            "absolute path": [("/escape", tarfile.REGTYPE, b"x", 0o644, "")],
            "symlink": [("payload/link", tarfile.SYMTYPE, b"", 0o777, "target")],
            "hardlink": [("payload/link", tarfile.LNKTYPE, b"", 0o644, "payload/file")],
            "device": [("payload/device", tarfile.CHRTYPE, b"", 0o600, "")],
            "duplicate name": [
                ("payload/file", tarfile.REGTYPE, b"first", 0o644, ""),
                ("payload/file", tarfile.REGTYPE, b"second", 0o644, ""),
            ],
            "file is parent": [
                ("payload", tarfile.REGTYPE, b"file", 0o644, ""),
                ("payload/child", tarfile.REGTYPE, b"child", 0o644, ""),
            ],
        }
        for label, members in bad_members.items():
            with self.subTest(label=label):
                archive = self.root / f"{label.replace(' ', '-')}.tar.gz"
                make_archive(archive, members)
                self.assert_archive_rejected(archive)

    def test_extract_normalizes_directory_executable_and_regular_file_modes(self) -> None:
        archive = self.root / "safe.tar.gz"
        payload = b"synthetic executable payload"
        make_archive(
            archive,
            [
                ("agenthub-webd-linux-x86_64", tarfile.DIRTYPE, b"", 0o4777, ""),
                ("agenthub-webd-linux-x86_64/bin/agenthub-webd", tarfile.REGTYPE, payload, 0o4751, ""),
                ("agenthub-webd-linux-x86_64/deploy/update.sh", tarfile.REGTYPE, b"#!/bin/sh\n", 0o0750, ""),
                ("agenthub-webd-linux-x86_64/deploy/config.example.toml", tarfile.REGTYPE, b"fixture\n", 0o2640, ""),
            ],
        )
        destination = self.root / "extract"
        destination.mkdir()
        destination.chmod(0o700)
        safety.extract(archive, destination)
        root = destination / "agenthub-webd-linux-x86_64"
        binary = destination / "agenthub-webd-linux-x86_64/bin/agenthub-webd"
        self.assertEqual(binary.read_bytes(), payload)
        self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE(root.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE((root / "bin").stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE((root / "deploy").stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE(binary.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE((root / "deploy/update.sh").stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE((root / "deploy/config.example.toml").stat().st_mode), 0o644)
        for path in (destination, root, root / "bin", binary, root / "deploy/update.sh", root / "deploy/config.example.toml"):
            self.assertEqual(stat.S_IMODE(path.stat().st_mode) & 0o7000, 0)

    def test_atomic_copy_replaces_file_and_applies_requested_mode(self) -> None:
        source = self.root / "source"
        target = self.root / "target"
        source.write_bytes(b"new content")
        target.write_bytes(b"old content")
        target.chmod(0o600)

        safety.atomic_copy(source, target, "0751")

        self.assertEqual(target.read_bytes(), b"new content")
        self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o751)

    def test_atomic_copy_failure_keeps_old_file_and_cleans_temporary_file(self) -> None:
        source = self.root / "source"
        target = self.root / "target"
        source.write_bytes(b"new content")
        target.write_bytes(b"old content")
        before = set(self.root.iterdir())

        with mock.patch.object(safety.os, "replace", side_effect=OSError("synthetic replace failure")):
            with self.assertRaises(OSError):
                safety.atomic_copy(source, target, "0640")

        self.assertEqual(target.read_bytes(), b"old content")
        self.assertEqual(set(self.root.iterdir()), before)

    def test_rollback_restores_old_files_removes_new_files_and_leaves_database_alone(self) -> None:
        transaction = self.root / "transaction"
        transaction.mkdir()
        managed = self.root / "managed"
        managed.mkdir()
        old_file = managed / "existing.conf"
        new_file = managed / "new-wrapper"
        database = self.root / "data/business.sqlite"
        database.parent.mkdir()
        old_file.write_text("before\n", encoding="utf-8")
        old_file.chmod(0o640)
        database.write_bytes(b"independent fixture database bytes")
        database.chmod(0o600)

        safety.snapshot(transaction, old_file)
        safety.snapshot(transaction, new_file)
        source = self.root / "replacement"
        source.write_text("after\n", encoding="utf-8")
        safety.atomic_copy(source, old_file, "0600")
        safety.atomic_copy(source, new_file, "0755")
        database_before = (database.read_bytes(), stat.S_IMODE(database.stat().st_mode))

        safety.rollback(transaction)

        self.assertEqual(old_file.read_text(encoding="utf-8"), "before\n")
        self.assertEqual(stat.S_IMODE(old_file.stat().st_mode), 0o640)
        self.assertFalse(new_file.exists())
        self.assertEqual((database.read_bytes(), stat.S_IMODE(database.stat().st_mode)), database_before)
        manifest = json.loads((transaction / "files.json").read_text(encoding="utf-8"))
        self.assertNotIn(str(database), [entry["target"] for entry in manifest])

    def test_install_main_rolls_back_after_service_start_failure_without_touching_system(self) -> None:
        managed = self.root / "managed"
        bin_dir = managed / "bin"
        share_dir = managed / "share"
        config_dir = managed / "config"
        data_dir = managed / "data"
        log_dir = managed / "log"
        systemd_dir = managed / "systemd"
        dropin_dir = systemd_dir / "agenthub-webd.service.d"
        for directory in (bin_dir, share_dir, config_dir, data_dir, log_dir, dropin_dir):
            directory.mkdir(parents=True, exist_ok=True)

        install_bin = bin_dir / "agenthub-webd"
        update_bin = bin_dir / "agenthub-webd-update"
        config = config_dir / "config.toml"
        env_file = config_dir / "env"
        unit = systemd_dir / "agenthub-webd.service"
        dropin = dropin_dir / "20-provider-session-paths.conf"
        safety_copy = share_dir / "install-safety.py"
        updater_key = share_dir / "updater.pub"
        wrappers = [
            bin_dir / "agenthub-codex-precheck",
            bin_dir / "agenthub-codex-update",
            bin_dir / "agenthub-codex-prune",
        ]
        database = data_dir / "agenthub.sqlite"
        database.write_bytes(b"business database fixture")
        database.chmod(0o600)

        preserved = {
            install_bin: (b"old executable", 0o751),
            config: (b"old config\n", 0o640),
            env_file: (b"old env\n", 0o640),
            unit: (b"old systemd unit\n", 0o644),
            dropin: (b"old drop-in\n", 0o644),
            safety_copy: (b"old helper\n", 0o644),
            updater_key: (b"old trusted public key\n", 0o644),
            wrappers[0]: (b"old wrapper\n", 0o755),
        }
        for path, (content, mode) in preserved.items():
            path.write_bytes(content)
            path.chmod(mode)
        for path in (update_bin, wrappers[1], wrappers[2]):
            self.assertFalse(path.exists())

        binary = self.root / "new-binary"
        binary.write_text("#!/bin/sh\nprintf '%s\\n' 'agenthub-webd 2.1.0'\n", encoding="utf-8")
        binary.chmod(0o755)
        next_config = self.root / "next-config"
        next_config.write_text('[codex]\nhome = "auto"\n', encoding="utf-8")
        next_env = self.root / "next-env"
        next_env.write_text("SYNTHETIC_KEY=value\n", encoding="utf-8")
        call_log = self.root / "systemctl.log"
        replacement_log = self.root / "replace.log"

        assignments = {
            "INSTALL_BIN": install_bin,
            "UPDATE_BIN": update_bin,
            "SHARE_DIR": share_dir,
            "CONFIG_DIR": config_dir,
            "CONFIG_FILE": config,
            "ENV_FILE": env_file,
            "DATA_DIR": data_dir,
            "LOG_DIR": log_dir,
            "BACKUP_DIR": data_dir / "backups",
            "NGINX_BACKUP_DIR": data_dir / "backups/nginx",
            "SYSTEMD_UNIT": unit,
            "SYSTEMD_DROPIN_DIR": dropin_dir,
            "PROVIDER_PATHS_DROPIN": dropin,
            "CODEX_PRECHECK_WRAPPER_BIN": wrappers[0],
            "CODEX_UPDATE_WRAPPER_BIN": wrappers[1],
            "CODEX_PRUNE_WRAPPER_BIN": wrappers[2],
            "NGINX_SNIPPET": managed / "nginx/snippets/agenthub.conf",
            "INSTALL_CONFIG_SOURCE": next_config,
            "INSTALL_ENV_SOURCE": next_env,
            "TEST_SYSTEMCTL_LOG": call_log,
            "TEST_REPLACE_LOG": replacement_log,
        }
        set_lines = "\n".join(f"{key}={shlex.quote(str(value))}" for key, value in assignments.items())
        harness = f"""#!/usr/bin/env bash
set -Eeuo pipefail
source {shlex.quote(str(INSTALL_PATH))}
require_root() {{ :; }}
install_dirs() {{ :; }}
replace_file() {{
  printf '%s\\n' "$2" >> "${{TEST_REPLACE_LOG}}"
  python3 "${{SAFETY}}" replace "${{TRANSACTION}}" "$1" "$2" "$3"
}}
retire_web_payload() {{ :; }}
install_codex_home_write_paths() {{ :; }}
install_config() {{
  replace_file "${{INSTALL_CONFIG_SOURCE}}" "${{CONFIG_FILE}}" 0640
  replace_file "${{INSTALL_ENV_SOURCE}}" "${{ENV_FILE}}" 0640
}}
systemctl() {{
  printf '%s\\n' "$*" >> "${{TEST_SYSTEMCTL_LOG}}"
  case "$*" in
    "is-active --quiet agenthub-webd.service") return 1 ;;
    "list-unit-files agenthub.service") return 1 ;;
    "restart agenthub-webd.service") return 77 ;;
  esac
  return 0
}}
{set_lines}
main --binary {shlex.quote(str(binary))}
"""
        harness_path = self.root / "install-failure-harness.sh"
        harness_path.write_text(harness, encoding="utf-8")
        result = subprocess.run(
            ["bash", str(harness_path)],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )

        self.assertEqual(result.returncode, 77, msg=f"stdout={result.stdout}\nstderr={result.stderr}")
        calls = call_log.read_text(encoding="utf-8").splitlines()
        self.assertIn("restart agenthub-webd.service", calls)
        replaced_targets = replacement_log.read_text(encoding="utf-8").splitlines()
        self.assertIn(str(updater_key), replaced_targets)
        for path, (content, mode) in preserved.items():
            with self.subTest(path=str(path)):
                self.assertEqual(path.read_bytes(), content)
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), mode)
        for path in (update_bin, wrappers[1], wrappers[2]):
            with self.subTest(path=str(path)):
                self.assertFalse(path.exists())
        self.assertEqual(database.read_bytes(), b"business database fixture")
        self.assertEqual(stat.S_IMODE(database.stat().st_mode), 0o600)

    def test_archive_install_requires_minisign(self) -> None:
        fake_bin = self.root / "missing-minisign-bin"
        fake_bin.mkdir()
        python_stub = fake_bin / "python3"
        python_stub.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        python_stub.chmod(0o755)
        install_marker = self.root / "install-started"
        harness = f"""#!/usr/bin/env bash
set -Eeuo pipefail
source {shlex.quote(str(INSTALL_PATH))}
require_root() {{ :; }}
install_dirs() {{ printf 'called\\n' > {shlex.quote(str(install_marker))}; }}
PATH={shlex.quote(str(fake_bin))}
main --archive {shlex.quote(str(self.root / 'unused-archive'))}
"""
        harness_path = self.root / "missing-minisign-harness.sh"
        harness_path.write_text(harness, encoding="utf-8")
        result = subprocess.run(
            ["/bin/bash", str(harness_path)],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required command not found: minisign", result.stderr)
        self.assertFalse(install_marker.exists(), "installer advanced without minisign")


    def test_binary_install_does_not_require_minisign(self) -> None:
        marker = self.root / "binary-install-started"
        harness = f"""set -Eeuo pipefail
source {shlex.quote(str(INSTALL_PATH))}
require_root() {{ :; }}
require_command() {{ [[ "$1" != minisign ]]; }}
systemctl() {{ return 1; }}
install_dirs() {{ printf 'called\\n' > {shlex.quote(str(marker))}; }}
install_payload() {{ exit 0; }}
main --binary fixture-binary
"""
        result = subprocess.run(["/bin/bash", "-c", harness], cwd=ROOT, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(marker.exists())

    def test_first_install_config_needs_no_http_domain_and_preserves_existing_secret(self) -> None:
        config = self.root / "config.toml"
        environment = self.root / "env"
        transaction = self.root / "transaction"
        transaction.mkdir()
        script = f"""set -Eeuo pipefail
source {shlex.quote(str(INSTALL_PATH))}
CONFIG_FILE={shlex.quote(str(config))}
ENV_FILE={shlex.quote(str(environment))}
TRANSACTION={shlex.quote(str(transaction))}
chown() {{ :; }}
parse_args --binary fixture
install_config
"""
        first = subprocess.run(["bash","-c",script],capture_output=True,text=True,check=False)
        self.assertEqual(first.returncode,0,first.stderr)
        original = environment.read_text()
        self.assertEqual(original.count("AGENTHUB_SECRET_KEY="),1)
        self.assertNotIn("listen =",config.read_text())
        second = subprocess.run(["bash","-c",script],capture_output=True,text=True,check=False)
        self.assertEqual(second.returncode,0,second.stderr)
        self.assertEqual(environment.read_text(),original)
        self.assertEqual(environment.stat().st_mode & 0o777,0o600)
        environment.write_text('AGENTHUB_SECRET_KEY=""\nFIXTURE=preserved\n')
        empty = subprocess.run(["bash","-c",script],capture_output=True,text=True,check=False)
        self.assertEqual(empty.returncode,0,empty.stderr)
        updated = environment.read_text()
        self.assertEqual(updated.count("AGENTHUB_SECRET_KEY="),1)
        self.assertIn("FIXTURE=preserved",updated)
        self.assertGreater(len(updated.splitlines()[0].partition("=")[2]),0)
        self.assertEqual(environment.stat().st_mode & 0o777,0o600)

    def test_bundled_ssh_bootstrap_reports_stage_and_never_installs_unverified_payload(self) -> None:
        validator = r"""import os, pathlib, sys
stage = sys.argv[1]
if stage == 'verify':
    if os.environ['FIXTURE_FAIL'] == 'signature': raise SystemExit(1)
    print('2.1.0')
if stage == 'extract':
    root = pathlib.Path(sys.argv[3]) / 'agenthub-webd-linux-x86_64/deploy'
    root.mkdir(parents=True)
    (root / 'install.sh').write_text('printf "installed\\n" >> "$FIXTURE_LOG"\n[[ "$FIXTURE_FAIL" != install ]]\n')
"""
        health = self.root / 'fixture-health'
        health.write_text('#!/bin/sh\n[ "$FIXTURE_FAIL" != health ]\n')
        health.chmod(0o755)
        source = (ROOT / "deploy/agenthub-webd/bootstrap.sh").read_text()
        source = source.replace('@VERSION@','2.1.0').replace('@PUBLIC_KEY@','fixture-public-key').replace('@VALIDATOR@',validator)
        source = source.replace('"${EUID}" -eq 0','0 -eq 0').replace('. /etc/os-release', ': # fixture OS is supplied by environment').replace('/usr/local/bin/agenthub-webd health',shlex.quote(str(health)))
        shim = r"""
uname() { if [[ "$1" == -s ]]; then echo Linux; else echo x86_64; fi; }
systemctl() { :; }
command() {
  if [[ "$*" == '-v minisign' ]]; then [[ "$FIXTURE_MISSING" == 0 ]]; else builtin command "$@"; fi
}
apt-get() { printf 'dependency\n' >> "$FIXTURE_LOG"; }
python3() {
  if [[ "$1" == -c && "$FIXTURE_FAIL" == dependencies && "$FIXTURE_MISSING" == 0 ]]; then return 1; fi
  command python3 "$@"
}
curl() {
  printf '%s\n' "$2" >> "$FIXTURE_LOG"
  [[ "$FIXTURE_FAIL" != download ]] || return 22
  : > "$4"
}
"""
        for failure, missing, distro in [('', '0','ubuntu'),('', '1','debian'),('download','0','ubuntu'),('signature','0','ubuntu'),('install','0','ubuntu'),('health','0','ubuntu'),('dependencies','1','other'),('dependencies','0','ubuntu')]:
            with self.subTest(failure=failure, missing=missing, distro=distro):
                log = self.root / 'bootstrap.log'
                log.unlink(missing_ok=True)
                env = {**os.environ,'FIXTURE_FAIL':failure,'FIXTURE_MISSING':missing,'FIXTURE_LOG':str(log),'ID':distro}
                result = subprocess.run(['bash','-c',shim + '\n' + source],env=env,capture_output=True,text=True,check=False,timeout=15)
                content = log.read_text() if log.exists() else ''
                if not failure:
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('installed',content)
                    self.assertIn('/releases/download/v2.1.0/',content)
                    self.assertEqual('dependency' in content,missing=='1')
                else:
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(f'AGENTHUB_INSTALL_ERROR:{failure}',result.stdout)
                    if failure in ('dependencies','download','signature'):
                        self.assertNotIn('installed',content)

    def test_update_handoff_and_readonly_paths_create_no_temporary_directory(self) -> None:
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        private_tmp = self.root / "tmp"
        private_tmp.mkdir()
        calls = self.root / "calls"
        for name, body in {
            "mktemp": 'printf "mktemp\\n" >> "$FIXTURE_CALLS"; exit 98',
            "systemd-run": 'printf "handoff\\n" >> "$FIXTURE_CALLS"; exit 0',
            "systemctl": "exit 0",
            "health": "exit 0",
        }.items():
            script = fake_bin / name
            script.write_text(f"#!/bin/sh\n{body}\n")
            script.chmod(0o755)
        source = UPDATE_PATH.read_text().replace("/usr/local/bin/agenthub-webd", str(fake_bin / "health"))
        updater = self.root / "update.sh"
        updater.write_text(source)
        environment = {**os.environ, "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}", "TMPDIR": str(private_tmp), "FIXTURE_CALLS": str(calls), "AGENTHUB_WEBD_UPDATE_TRANSIENT": "0"}
        for args in (["--help"], ["--precheck"], ["v1.2.3"]):
            result = subprocess.run(["bash", str(updater), *args], env=environment, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls.read_text(), "handoff\n")
        self.assertEqual(list(private_tmp.iterdir()), [])

    def test_update_download_failures_stop_before_systemd_or_installer(self) -> None:
        real_id = shutil.which("id")
        self.assertIsNotNone(real_id)
        for fail_at in (1, 2):
            with self.subTest(fail_at=fail_at):
                fixture = self.root / f"update-{fail_at}"
                fake_bin = fixture / "bin"
                fake_bin.mkdir(parents=True)
                private_tmp = fixture / "tmp"
                private_tmp.mkdir()
                helper = fixture / "install-safety.py"
                helper.write_text("# fixture helper; download is expected to fail first\n", encoding="utf-8")
                public_key = fixture / "updater.pub"
                public_key.write_text("fixture public key\n", encoding="utf-8")
                update_text = UPDATE_PATH.read_text(encoding="utf-8")
                for name, installed, replacement in (
                    ("SAFETY", '/usr/share/agenthub-webd/install-safety.py', helper),
                    ("PUBLIC_KEY", '/usr/share/agenthub-webd/updater.pub', public_key),
                ):
                    line = f'{name}="{installed}"'
                    self.assertEqual(update_text.count(line), 1)
                    update_text = update_text.replace(line, f"{name}={shlex.quote(str(replacement))}")
                update_script = fixture / "update.sh"
                update_script.write_text(update_text, encoding="utf-8")
                update_script.chmod(0o755)
                call_log = fixture / "calls.jsonl"
                id_script = fake_bin / "id"
                id_script.write_text(
                    "#!/bin/sh\n"
                    "if [ \"${1:-}\" = \"-u\" ]; then printf '0\\n'; exit 0; fi\n"
                    f"exec {shlex.quote(str(real_id))} \"$@\"\n",
                    encoding="utf-8",
                )
                id_script.chmod(0o755)
                minisign_stub = fake_bin / "minisign"
                minisign_stub.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
                minisign_stub.chmod(0o755)
                curl_script = fake_bin / "curl"
                curl_script.write_text(
                    f"#!{sys.executable}\n"
                    "import json, os, pathlib, sys\n"
                    "log = pathlib.Path(os.environ['TEST_FAKE_CALL_LOG'])\n"
                    "calls = log.read_text().splitlines() if log.exists() else []\n"
                    "calls.append(json.dumps(sys.argv[1:]))\n"
                    "log.write_text('\\n'.join(calls) + '\\n')\n"
                    "if len(calls) == int(os.environ['TEST_FAIL_CURL_AT']):\n"
                    "    raise SystemExit(22)\n"
                    "output = pathlib.Path(sys.argv[sys.argv.index('-o') + 1])\n"
                    "output.write_bytes(b'synthetic invalid archive')\n",
                    encoding="utf-8",
                )
                curl_script.chmod(0o755)
                for command_name in ("systemctl", "systemd-run"):
                    command = fake_bin / command_name
                    command.write_text(
                        "#!/bin/sh\n"
                        f"printf '%s\\n' {shlex.quote(command_name)} >> \"$TEST_FAKE_CALL_LOG\"\n"
                        "exit 97\n",
                        encoding="utf-8",
                    )
                    command.chmod(0o755)

                environment = os.environ.copy()
                environment.update(
                    {
                        "PATH": f"{fake_bin}{os.pathsep}{environment.get('PATH', '')}",
                        "AGENTHUB_WEBD_UPDATE_TRANSIENT": "1",
                        "AGENTHUB_WEBD_REPO": "example.invalid/fixture",
                        "TEST_FAKE_CALL_LOG": str(call_log),
                        "TEST_FAIL_CURL_AT": str(fail_at),
                        "TMPDIR": str(private_tmp),
                    }
                )
                result = subprocess.run(
                    ["/bin/bash", str(update_script), "--version", "latest"],
                    cwd=ROOT,
                    env=environment,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=30,
                    check=False,
                )

                self.assertEqual(result.returncode, 22, msg=f"stdout={result.stdout}\nstderr={result.stderr}")
                self.assertEqual(list(private_tmp.iterdir()), [], "failed downloads must clean their staging directory")
                calls = [json.loads(line) for line in call_log.read_text(encoding="utf-8").splitlines()]
                self.assertEqual(len(calls), fail_at)
                self.assertTrue(all(call and call[0] == "-fL" for call in calls))
                self.assertFalse(any(call_name in str(calls) for call_name in ("systemctl", "systemd-run")))


if __name__ == "__main__":
    unittest.main(verbosity=2)
