#!/usr/bin/env python3
"""Exercise Tauri-wrapped minisign release signatures with disposable keys."""

from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SAFETY_PATH = ROOT / "deploy/agenthub-webd/install-safety.py"
UPDATE_PATH = ROOT / "deploy/agenthub-webd/update.sh"
TAURI_CLI = ROOT / "webui/node_modules/.bin/tauri"
TAURI_CONFIG = ROOT / "src-tauri/tauri.conf.json"
UPDATER_PUB = ROOT / "deploy/agenthub-webd/updater.pub"
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("install_safety", SAFETY_PATH)
if spec is None or spec.loader is None:
    raise RuntimeError("could not load installer safety module")
safety = importlib.util.module_from_spec(spec)
spec.loader.exec_module(safety)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_checksum(archive: Path, checksum: Path) -> None:
    checksum.write_text(f"{sha256(archive)}  {archive.name}\n", encoding="ascii")


class UpdaterTrustRootTests(unittest.TestCase):
    def test_installed_updater_key_matches_tauri_updater_pubkey(self) -> None:
        config = json.loads(TAURI_CONFIG.read_text(encoding="utf-8"))
        configured = base64.b64decode(config["plugins"]["updater"]["pubkey"], validate=True)
        self.assertEqual(UPDATER_PUB.read_bytes().strip(), configured.strip())


class TauriMinisignVerificationTests(unittest.TestCase):
    VERSION = "1.2.23"

    @classmethod
    def setUpClass(cls) -> None:
        if not TAURI_CLI.is_file() or not os.access(TAURI_CLI, os.X_OK):
            raise RuntimeError("installed Tauri CLI is required for signature tests")
        if shutil.which("minisign") is None:
            raise RuntimeError("installed minisign is required for signature tests")

        cls.temp = tempfile.TemporaryDirectory(prefix="agenthub-signature-tests-")
        cls.root = Path(cls.temp.name)
        try:
            cls.private_key, cls.public_key_bytes = cls.generate_keypair("trusted")
            cls.wrong_private_key, cls.wrong_public_key_bytes = cls.generate_keypair("wrong")
            cls.public_key = cls.root / "trusted.pub"
            cls.public_key.write_bytes(cls.public_key_bytes)
            cls.wrong_public_key = cls.root / "wrong.pub"
            cls.wrong_public_key.write_bytes(cls.wrong_public_key_bytes)
            cls.archive = cls.root / "agenthub-webd-linux-x86_64.tar.gz"
            cls.archive.write_bytes(b"synthetic release archive used only for signature testing\n")
            cls.checksum = cls.root / f"{cls.archive.name}.sha256"
            write_checksum(cls.archive, cls.checksum)

            environment = os.environ.copy()
            environment.pop("TAURI_SIGNING_PRIVATE_KEY_PATH", None)
            environment["TAURI_SIGNING_PRIVATE_KEY"] = cls.private_key.read_text(encoding="utf-8")
            environment["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"] = ""
            result = subprocess.run(
                [
                    str(TAURI_CLI),
                    "signer",
                    "sign",
                    "--app-version",
                    cls.VERSION,
                    str(cls.archive),
                ],
                cwd=ROOT,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=90,
                check=False,
            )
            if result.returncode != 0:
                raise RuntimeError(f"Tauri signer failed with exit code {result.returncode}")
            cls.signature = Path(f"{cls.archive}.sig")
            if not cls.signature.is_file():
                raise RuntimeError("Tauri signer did not create a signature")
        except BaseException:
            cls.temp.cleanup()
            raise

    @classmethod
    def generate_keypair(cls, name: str) -> tuple[Path, bytes]:
        private_key = cls.root / f"{name}.key"
        environment = os.environ.copy()
        for variable in (
            "TAURI_SIGNING_PRIVATE_KEY",
            "TAURI_SIGNING_PRIVATE_KEY_PATH",
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        ):
            environment.pop(variable, None)
        result = subprocess.run(
            [
                str(TAURI_CLI),
                "signer",
                "generate",
                "--write-keys",
                str(private_key),
                "--ci",
            ],
            cwd=ROOT,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=90,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(f"Tauri key generation failed with exit code {result.returncode}")
        public_wrapper = Path(f"{private_key}.pub")
        if not private_key.is_file() or not public_wrapper.is_file():
            raise RuntimeError("Tauri signer did not create a key pair")
        try:
            public_key = base64.b64decode(public_wrapper.read_bytes().strip(), validate=True)
            public_key.decode("utf-8")
        except (ValueError, UnicodeError):
            raise RuntimeError("Tauri public key wrapper was invalid") from None
        return private_key, public_key

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temp.cleanup()

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="agenthub-signature-case-")
        self.root = Path(self.temp.name)
        self.archive = self.root / TauriMinisignVerificationTests.archive.name
        shutil.copyfile(self.__class__.archive, self.archive)
        self.checksum = self.root / f"{self.archive.name}.sha256"
        shutil.copyfile(self.__class__.checksum, self.checksum)
        self.signature = self.root / f"{self.archive.name}.sig"
        shutil.copyfile(self.__class__.signature, self.signature)
        self.public_key = self.root / "updater.pub"
        shutil.copyfile(self.__class__.public_key, self.public_key)
        self.wrong_public_key = self.root / "wrong.pub"
        shutil.copyfile(self.__class__.wrong_public_key, self.wrong_public_key)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_tauri_signature_authenticates_the_signed_version(self) -> None:
        wrapped = base64.b64decode(self.signature.read_bytes().strip(), validate=True).decode("utf-8")
        comments = [line for line in wrapped.splitlines() if line.startswith("trusted comment: ")]
        self.assertEqual(len(comments), 1)
        self.assertIn(f"version:{self.VERSION}", comments[0])
        self.assertEqual(
            safety.verify(self.archive, self.checksum, self.signature, self.public_key),
            self.VERSION,
        )
        self.assertEqual(
            safety.verify(self.archive, self.checksum, self.signature, self.public_key, f"v{self.VERSION}"),
            self.VERSION,
        )

    def test_modified_archive_with_matching_recomputed_channel_checksum_is_rejected(self) -> None:
        self.archive.write_bytes(self.archive.read_bytes() + b"tampered\n")
        write_checksum(self.archive, self.checksum)
        with self.assertRaises(ValueError):
            safety.verify(self.archive, self.checksum, self.signature, self.public_key)

    def test_signature_from_another_public_key_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            safety.verify(self.archive, self.checksum, self.signature, self.wrong_public_key)

    def test_missing_signature_and_missing_public_key_are_rejected(self) -> None:
        with self.subTest("missing signature"):
            with self.assertRaises(ValueError):
                safety.verify(self.archive, self.checksum, self.root / "missing.sig", self.public_key)
        with self.subTest("missing public key"):
            with self.assertRaises(ValueError):
                safety.verify(self.archive, self.checksum, self.signature, self.root / "missing.pub")

    def test_malformed_signature_and_public_key_are_rejected(self) -> None:
        malformed_signature = self.root / "malformed.sig"
        malformed_signature.write_bytes(b"not base64 !\n")
        with self.subTest("malformed signature"):
            with self.assertRaises(ValueError):
                safety.verify(self.archive, self.checksum, malformed_signature, self.public_key)

        malformed_key = self.root / "malformed.pub"
        malformed_key.write_text("not a minisign public key\n", encoding="ascii")
        with self.subTest("malformed public key"):
            with self.assertRaises(ValueError):
                safety.verify(self.archive, self.checksum, self.signature, malformed_key)

    def test_signed_version_mismatch_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            safety.verify(self.archive, self.checksum, self.signature, self.public_key, "9.9.9")

    def test_missing_minisign_binary_is_rejected(self) -> None:
        with mock.patch.object(safety.shutil, "which", return_value=None):
            with self.assertRaises(ValueError):
                safety.verify(self.archive, self.checksum, self.signature, self.public_key)

    def test_update_rejects_tampering_before_extract_or_install_and_cleans_temp_files(self) -> None:
        case = self.root / "update-case"
        case.mkdir()
        private_tmp = case / "tmp"
        private_tmp.mkdir()
        fixture_archive = case / self.archive.name
        fixture_archive.write_bytes(self.archive.read_bytes() + b"tampered update payload\n")
        fixture_checksum = case / f"{fixture_archive.name}.sha256"
        write_checksum(fixture_archive, fixture_checksum)
        fixture_signature = case / f"{fixture_archive.name}.sig"
        shutil.copyfile(self.signature, fixture_signature)
        fake_bin = case / "bin"
        fake_bin.mkdir()
        verify_log = case / "safety-operations.jsonl"
        curl_log = case / "curl-calls.jsonl"
        installer_log = case / "installer-called"
        systemd_log = case / "systemd-called"

        curl_script = fake_bin / "curl"
        curl_script.write_text(
            f"#!{sys.executable}\n"
            "import json, os, pathlib, sys\n"
            "output = pathlib.Path(sys.argv[sys.argv.index('-o') + 1])\n"
            "if output.name.endswith('.sha256'):\n"
            "    source = pathlib.Path(os.environ['TEST_FIXTURE_CHECKSUM'])\n"
            "elif output.name.endswith('.sig'):\n"
            "    source = pathlib.Path(os.environ['TEST_FIXTURE_SIGNATURE'])\n"
            "else:\n"
            "    source = pathlib.Path(os.environ['TEST_FIXTURE_ARCHIVE'])\n"
            "output.write_bytes(source.read_bytes())\n"
            "with pathlib.Path(os.environ['TEST_CURL_LOG']).open('a', encoding='utf-8') as log:\n"
            "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n",
            encoding="utf-8",
        )
        curl_script.chmod(0o755)

        id_script = fake_bin / "id"
        id_script.write_text(
            "#!/bin/sh\n"
            "if [ \"$1\" = \"-u\" ]; then printf '0\\n'; exit 0; fi\n"
            "exit 97\n",
            encoding="utf-8",
        )
        id_script.chmod(0o755)

        python_shim = fake_bin / "python3"
        python_shim.write_text(
            f"#!{sys.executable}\n"
            "import json, os, pathlib, sys\n"
            "if len(sys.argv) > 2:\n"
            "    with pathlib.Path(os.environ['TEST_VERIFY_LOG']).open('a', encoding='utf-8') as log:\n"
            "        log.write(json.dumps({'operation': sys.argv[2]}) + '\\n')\n"
            "real = os.environ['TEST_REAL_PYTHON']\n"
            "os.execv(real, [real, *sys.argv[1:]])\n",
            encoding="utf-8",
        )
        python_shim.chmod(0o755)

        for command in ("systemd-run", "systemctl"):
            stub = fake_bin / command
            stub.write_text(
                "#!/bin/sh\n"
                f"printf '%s\\n' {shlex.quote(command)} >> \"$TEST_SYSTEMD_LOG\"\n"
                "exit 97\n",
                encoding="utf-8",
            )
            stub.chmod(0o755)

        update_text = UPDATE_PATH.read_text(encoding="utf-8")
        for name, installed, replacement in (
            ("SAFETY", "/usr/share/agenthub-webd/install-safety.py", SAFETY_PATH),
            ("PUBLIC_KEY", "/usr/share/agenthub-webd/updater.pub", self.public_key),
        ):
            line = f'{name}="{installed}"'
            self.assertEqual(update_text.count(line), 1)
            update_text = update_text.replace(line, f"{name}={shlex.quote(str(replacement))}")

        install_call = (
            '"' + chr(36) + '{ROOT}/deploy/install.sh" --archive "'
            + chr(36) + '{TMP_DIR}/' + chr(36) + '{ASSET}" --no-enable'
        )
        self.assertEqual(update_text.count(install_call), 1)
        update_text = update_text.replace(
            install_call,
            f"printf '%s\\n' called > {shlex.quote(str(installer_log))}",
        )
        update_script = case / "update.sh"
        update_script.write_text(update_text, encoding="utf-8")
        update_script.chmod(0o755)

        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{fake_bin}{os.pathsep}{environment.get('PATH', '')}",
                "TMPDIR": str(private_tmp),
                "AGENTHUB_WEBD_UPDATE_TRANSIENT": "1",
                "AGENTHUB_WEBD_REPO": "example.invalid/fixture",
                "TEST_FIXTURE_ARCHIVE": str(fixture_archive),
                "TEST_FIXTURE_CHECKSUM": str(fixture_checksum),
                "TEST_FIXTURE_SIGNATURE": str(fixture_signature),
                "TEST_CURL_LOG": str(curl_log),
                "TEST_VERIFY_LOG": str(verify_log),
                "TEST_REAL_PYTHON": sys.executable,
                "TEST_SYSTEMD_LOG": str(systemd_log),
            }
        )
        result = subprocess.run(
            ["/bin/bash", str(update_script), "--version", "latest"],
            cwd=ROOT,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=60,
            check=False,
        )

        self.assertNotEqual(result.returncode, 0, msg=f"update unexpectedly succeeded: {result.stdout}")
        operations = [
            json.loads(line)["operation"]
            for line in verify_log.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(operations, ["verify"], "failed authentication must stop before extract")
        self.assertEqual(len(curl_log.read_text(encoding="utf-8").splitlines()), 3)
        self.assertFalse(installer_log.exists(), "failed authentication must stop before install")
        self.assertFalse(systemd_log.exists(), "failed authentication must stop before systemd restart")
        self.assertEqual(list(private_tmp.iterdir()), [], "update and signature temporary files must be cleaned")


if __name__ == "__main__":
    unittest.main(verbosity=2)
