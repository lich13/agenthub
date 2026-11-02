#!/usr/bin/env python3
"""Internal installer primitives; operate only on an explicitly owned transaction."""
import base64
import hashlib
import json
import os
import re
import subprocess
from pathlib import Path, PurePosixPath
import shutil
import sys
import tarfile
import tempfile


def extract(archive, destination):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink() or any(destination.iterdir()):
        raise ValueError("extraction requires an empty owned directory")
    destination.chmod(0o755)
    with tarfile.open(archive, "r:gz") as source:
        members = source.getmembers()
        seen = set()
        for member in members:
            path = PurePosixPath(member.name)
            if (path.is_absolute() or ".." in path.parts or not path.parts
                    or "\\" in member.name or str(path) in seen
                    or not (member.isdir() or member.isfile())):
                raise ValueError("unsafe archive member")
            seen.add(str(path))
        # Do not allow a regular member to masquerade as a parent directory.
        files = {PurePosixPath(m.name) for m in members if m.isfile()}
        if any(any(parent in files for parent in PurePosixPath(m.name).parents) for m in members):
            raise ValueError("archive file is also a parent directory")
        for member in members:
            target = destination / member.name
            for parent in reversed(target.relative_to(destination).parents):
                directory = destination / parent
                directory.mkdir(exist_ok=True)
                directory.chmod(0o755)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                target.chmod(0o755)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as incoming, target.open("xb") as output:
                    shutil.copyfileobj(incoming, output)
                target.chmod(0o755 if member.mode & 0o111 else 0o644)


def verify_checksum(archive, checksum):
    archive = Path(archive)
    lines = Path(checksum).read_text().splitlines()
    if len(lines) != 1:
        raise ValueError("expected one release checksum")
    parts = lines[0].split()
    if len(parts) != 2 or parts[1].lstrip("*") != archive.name:
        raise ValueError("checksum does not name the downloaded archive")
    with archive.open("rb") as stream:
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        actual = digest.hexdigest()
    if actual != parts[0].lower():
        raise ValueError("release checksum mismatch")


def verify(archive, checksum, signature, public_key, expected_version="latest"):
    """Trust only an installed key and validator; never execute downloaded content."""
    verify_checksum(archive, checksum)
    archive, signature, public_key = map(Path, (archive, signature, public_key))
    if not public_key.is_file() or public_key.is_symlink() or not signature.is_file():
        raise ValueError("release signature or trust root unavailable")
    validator = shutil.which("minisign")
    if validator is None:
        raise ValueError("release signature validator unavailable")
    if signature.stat().st_size > 16 * 1024:
        raise ValueError("invalid release signature")
    try:
        decoded = base64.b64decode(signature.read_bytes().strip(), validate=True)
        text = decoded.decode("utf-8")
    except (ValueError, UnicodeError):
        raise ValueError("invalid release signature") from None
    with tempfile.TemporaryDirectory(prefix="agenthub-signature-") as temporary:
        minisig = Path(temporary) / "release.minisig"
        minisig.write_bytes(decoded)
        try:
            result = subprocess.run([validator, "-Vm", str(archive.resolve()), "-p", str(public_key.resolve()),
                                     "-x", str(minisig), "-q"], stdout=subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, timeout=60, check=False)
        except (OSError, subprocess.TimeoutExpired):
            raise ValueError("release signature validation failed") from None
        if result.returncode != 0:
            raise ValueError("release signature validation failed")
    # Read metadata only after the global signature has authenticated the comment.
    comments = [line for line in text.splitlines() if line.startswith("trusted comment: ")]
    if len(comments) != 1:
        raise ValueError("signed release version unavailable")
    versions = re.findall(r"(?:^|[\t ])version:([0-9]+\.[0-9]+\.[0-9]+)(?=$|[\t ])", comments[0])
    if len(versions) != 1:
        raise ValueError("signed release version unavailable")
    version = versions[0]
    if expected_version != "latest" and expected_version.removeprefix("v") != version:
        raise ValueError("signed release version mismatch")
    return version


def validate_payload(root, signed_version):
    root = Path(root)
    required = ("bin/agenthub-webd", "deploy/install.sh", "deploy/update.sh", "deploy/install-safety.py", "deploy/updater.pub")
    if sorted(p.name for p in root.parent.iterdir()) != ["agenthub-webd-linux-x86_64"] or any(not (root / p).is_file() for p in required):
        raise ValueError("invalid release layout")
    if (root / "webui").exists() or (root / "deploy/web-update.sh").exists():
        raise ValueError("retired release layout")
    with (root / "bin/agenthub-webd").open("rb") as binary:
        header = binary.read(20)
    if len(header) != 20 or header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
        raise ValueError("invalid release platform")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", signed_version):
        raise ValueError("invalid signed version")
    # Preserve the previous updater's rejection of retired web-authentication releases.
    if tuple(map(int, signed_version.split("."))) < (1, 2, 0):
        raise ValueError("refusing release with retired web authentication")
    # This binary is allowed to execute only after verify() and layout checks.
    result = subprocess.run([str(root / "bin/agenthub-webd"), "--version"], capture_output=True, timeout=30, check=False)
    if result.returncode != 0 or result.stdout.decode("utf-8").strip() != f"agenthub-webd {signed_version}":
        raise ValueError("release binary version mismatch")


def atomic_copy(source, target, mode=None):
    target = Path(target)
    if target.is_symlink() or (target.exists() and not target.is_file()):
        raise ValueError("managed target must be a regular file")
    descriptor, temporary = tempfile.mkstemp(prefix=".agenthub-install-", dir=target.parent)
    os.close(descriptor)
    try:
        shutil.copy2(source, temporary)
        if mode is not None:
            os.chmod(temporary, int(mode, 8))
        else:
            stat = Path(source).stat()
            if os.geteuid() == 0:
                os.chown(temporary, stat.st_uid, stat.st_gid)
        with open(temporary, "rb") as stream:
            os.fsync(stream.fileno())
        os.replace(temporary, target)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def snapshot(transaction, target):
    transaction, target = Path(transaction), Path(target)
    manifest = transaction / "files.json"
    entries = json.loads(manifest.read_text()) if manifest.exists() else []
    if str(target) in [entry["target"] for entry in entries]:
        return
    if target.is_symlink() or (target.exists() and not target.is_file()):
        raise ValueError("managed target must be a regular file")
    backup = str(len(entries)) if target.exists() else None
    if backup is not None:
        shutil.copy2(target, transaction / backup)
        if os.geteuid() == 0:
            stat = target.stat()
            os.chown(transaction / backup, stat.st_uid, stat.st_gid)
    entries.append({"target": str(target), "backup": backup})
    temporary = transaction / "files.next"
    temporary.write_text(json.dumps(entries))
    os.replace(temporary, manifest)


def rollback(transaction):
    transaction = Path(transaction)
    manifest = transaction / "files.json"
    if not manifest.exists():
        return
    entries = json.loads(manifest.read_text())
    for entry in reversed(entries):
        target = Path(entry["target"])
        if entry["backup"] is None:
            if target.is_symlink() or (target.exists() and not target.is_file()):
                raise ValueError("managed target identity changed")
            target.unlink(missing_ok=True)
        else:
            atomic_copy(transaction / entry["backup"], target)


def main():
    operation, *args = sys.argv[1:]
    if operation == "extract":
        extract(*args)
    elif operation == "verify":
        print(verify(*args))
    elif operation == "validate-payload":
        validate_payload(*args)
    elif operation == "snapshot":
        snapshot(*args)
    elif operation == "replace":
        transaction, source, target, mode = args
        snapshot(transaction, target)
        atomic_copy(source, target, mode)
    elif operation == "rollback":
        rollback(*args)
    else:
        raise ValueError("unknown installer operation")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, tarfile.TarError, EOFError, KeyError, TypeError, UnicodeError, subprocess.SubprocessError):
        raise SystemExit("installer file validation or replacement failed") from None
