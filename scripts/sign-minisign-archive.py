#!/usr/bin/env python3
"""Non-interactive Minisign archive signer compatible with Tauri updater & install-safety.py."""

from __future__ import annotations

import argparse
import base64
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


SEMVER_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")


def decode_secret_key_text(raw: str) -> str:
    stripped = raw.strip()
    if not stripped:
        raise ValueError("signing private key is empty")
    if stripped.startswith("untrusted comment:"):
        return stripped + "\n"
    try:
        decoded = base64.b64decode(stripped, validate=True).decode("utf-8").strip()
    except (ValueError, UnicodeError) as exc:
        raise ValueError("signing private key is neither raw minisign text nor valid Base64") from exc
    if not decoded.startswith("untrusted comment:"):
        raise ValueError("decoded signing private key is missing minisign header")
    return decoded + "\n"


def is_encrypted_minisign_secret_key(secret_text: str) -> bool:
    lines = [line.strip() for line in secret_text.splitlines() if line.strip()]
    if len(lines) < 2:
        raise ValueError("malformed minisign secret key")
    key_bytes = base64.b64decode(lines[1], validate=True)
    if len(key_bytes) < 4 or key_bytes[:2] != b"Ed":
        raise ValueError("unsupported minisign secret key algorithm")
    return key_bytes[2:4] != b"\x00\x00"


def load_secret_key_from_env_or_path(key_path: Path | None) -> str:
    if key_path is not None:
        return decode_secret_key_text(key_path.read_text(encoding="utf-8"))
    env_key = os.environ.get("AGENTHUB_SIGNING_PRIVATE_KEY", "").strip()
    if env_key:
        return decode_secret_key_text(env_key)
    env_path = os.environ.get("AGENTHUB_SIGNING_PRIVATE_KEY_PATH", "").strip()
    if env_path:
        return decode_secret_key_text(Path(env_path).read_text(encoding="utf-8"))
    raise ValueError("missing AGENTHUB_SIGNING_PRIVATE_KEY or AGENTHUB_SIGNING_PRIVATE_KEY_PATH")


def sign_archive(
    archive: Path,
    version: str,
    public_key: Path,
    output_sig: Path | None = None,
    secret_key_path: Path | None = None,
) -> Path:
    normalized_version = version.removeprefix("v")
    if not SEMVER_RE.fullmatch(normalized_version):
        raise ValueError(f"invalid semantic version: {version}")
    if not archive.is_file():
        raise ValueError(f"archive does not exist: {archive}")
    if not public_key.is_file():
        raise ValueError(f"public key does not exist: {public_key}")

    minisign = shutil.which("minisign")
    if minisign is None:
        raise ValueError("minisign binary is not installed in PATH")

    secret_text = load_secret_key_from_env_or_path(secret_key_path)
    encrypted = is_encrypted_minisign_secret_key(secret_text)
    password = os.environ.get("AGENTHUB_SIGNING_PRIVATE_KEY_PASSWORD", "")

    trusted_comment = (
        f"timestamp:{int(time.time())}\tfile:{archive.name}\tversion:{normalized_version}"
    )
    target_sig = output_sig if output_sig is not None else Path(f"{archive}.sig")

    with tempfile.TemporaryDirectory(prefix="agenthub-minisign-") as temp_dir:
        temp_root = Path(temp_dir)
        os.chmod(temp_root, 0o700)
        sec_file = temp_root / "signing.key"
        fd = os.open(sec_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(secret_text)

        minisig_file = temp_root / f"{archive.name}.minisig"
        sign_cmd = [
            minisign,
            "-Sm",
            str(archive.resolve()),
            "-s",
            str(sec_file),
            "-x",
            str(minisig_file),
            "-t",
            trusted_comment,
        ]
        stdin_bytes = f"{password}\n".encode("utf-8") if encrypted else None
        sign_res = subprocess.run(
            sign_cmd,
            input=stdin_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=60,
            check=False,
        )
        if sign_res.returncode != 0 or not minisig_file.is_file():
            err_msg = sign_res.stderr.decode("utf-8", errors="replace").strip()
            raise RuntimeError(f"minisign signing failed (rc={sign_res.returncode}): {err_msg}")

        # Immediately verify against updater.pub before emitting .sig
        verify_res = subprocess.run(
            [
                minisign,
                "-Vm",
                str(archive.resolve()),
                "-p",
                str(public_key.resolve()),
                "-x",
                str(minisig_file),
                "-q",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=60,
            check=False,
        )
        if verify_res.returncode != 0:
            raise RuntimeError("generated minisign signature failed verification against public key")

        raw_minisig = minisig_file.read_bytes()
        encoded_sig = base64.b64encode(raw_minisig).decode("ascii") + "\n"
        target_sig.write_text(encoded_sig, encoding="ascii")

    return target_sig


def main() -> int:
    parser = argparse.ArgumentParser(description="Sign release archive with Minisign.")
    parser.add_argument("--archive", required=True, type=Path, help="Path to archive file")
    parser.add_argument("--version", required=True, help="Release semantic version")
    parser.add_argument(
        "--public-key",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "deploy/agenthub-webd/updater.pub",
        help="Minisign public key for immediate verification",
    )
    parser.add_argument("--output", type=Path, default=None, help="Output .sig file path")
    parser.add_argument("--secret-key-file", type=Path, default=None, help="Optional secret key file")
    args = parser.parse_args()

    out = sign_archive(
        archive=args.archive,
        version=args.version,
        public_key=args.public_key,
        output_sig=args.output,
        secret_key_path=args.secret_key_file,
    )
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
