#!/usr/bin/env python3
"""Check source, Git objects/identities and unpacked release payloads without echoing matches."""
import argparse
import ipaddress
import json
import re
import subprocess
import sys
from pathlib import Path

PUBLIC_HOSTS = {
    "github.com", "api.github.com", "raw.githubusercontent.com", "objects.githubusercontent.com",
    "api.day.app", "challenges.cloudflare.com", "registry.npmjs.org", "registry.yarnpkg.com",
    "schema.tauri.app", "json-schema.org", "schemas.android.com", "www.apple.com", "www.w3.org",
    "docs.github.com", "docs.x.ai", "x.ai", "docs.rs", "crates.io", "static.crates.io",
    "localhost", "ipc.localhost", "tauri.localhost", "demo.agenthub.local",
}
EXAMPLE_HOSTS = ("example.com", "example.org", "example.net", "example.invalid", "localhost", "local", "test")
RULES = {
    "private_home": re.compile(r"/(?:Users|home)/([^/\s\"'<>`]+)"),
    "ipv4": re.compile(r"(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?![\w.])"),
    "email": re.compile(r"(?<![\w@])([\w.+-]+)@([a-zA-Z0-9.-]+\.[a-zA-Z]{2,})(?![\w.])"),
    "url": re.compile(r"https?://([a-zA-Z0-9.-]+)(?=[:/\s\"'`<>)]|$)"),
    "secret": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----\s+[A-Za-z0-9+/=]{32,}|\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|sk-[A-Za-z0-9_-]{32,})\b"),
    "personal_trace": re.compile(
        "|".join(
            [
                r"cc-switch" + r"-lite",
                r"CC_SWITCH" + r"_LITE",
                r"/srv/" + r"hermes\b",
                r"\bhermes" + r"\s+agent\b",
                r"\u817e\u8baf" + r"\u4e91",
                r"\btencent-" + r"cloud(?:-ts)?\b",
                r"\u4e91" + r"\u673a",
                r"\bxian" + r"bao\b",
            ]
        ),
        re.IGNORECASE,
    ),
}
EXAMPLE_USERS = {"example", "test", "alice", "runner", "codex", "ubuntu", "user", "*"}


def allowed_host(host):
    host = host.lower().rstrip(".")
    return host in PUBLIC_HOSTS or any(host == suffix or host.endswith("." + suffix) for suffix in EXAMPLE_HOSTS)


def problems(data, private_values, scan_endpoint_literals=True):
    native_binary = data[:4] in (b"\x7fELF", b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe")
    text = data.decode("utf-8", errors="replace").replace(r"\.", ".")
    found = set()
    for match in RULES["private_home"].finditer(text):
        if match.group(1) not in EXAMPLE_USERS and not any(c in match.group(1) for c in "$[{(\\"):
            found.add("private_home")
    # Native machine code and non-terminated string tables include dependency
    # license contacts and concatenated protocol strings. Scan their private
    # values, paths, public IPv4 addresses and credentials; source, Git
    # identities and text payloads also validate endpoints and emails.
    for match in RULES["ipv4"].finditer(text):
        try:
            address = ipaddress.ip_address(match.group())
            if address.is_global:
                found.add("public_ip")
        except ValueError:
            pass
    for match in (() if native_binary else RULES["email"].finditer(text)):
        if re.fullmatch(r"\d+x\d+@\d+x\.png", match.group()):
            continue
        host = match.group(2).lower()
        if host != "users.noreply.github.com" and not any(host == h or host.endswith("." + h) for h in EXAMPLE_HOSTS):
            found.add("private_email")
    for match in (() if native_binary or not scan_endpoint_literals else RULES["url"].finditer(text)):
        host = match.group(1)
        try:
            if ipaddress.ip_address(host).is_loopback:
                continue
        except ValueError:
            pass
        if not allowed_host(host):
            found.add("unreviewed_endpoint")
    if RULES["secret"].search(text):
        found.add("credential")
    if not native_binary and RULES["personal_trace"].search(text):
        found.add("personal_trace")
    if any(value and value.encode() in data for value in private_values):
        found.add("private_value")
    return sorted(found)


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], stderr=subprocess.DEVNULL)


def scan():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--git-objects", action="store_true", help="Scan every reachable blob and commit/tag identity")
    parser.add_argument("--payload", type=Path, action="append", default=[], help="Unpacked release payload; scan every regular file")
    parser.add_argument("--private-values", type=Path, help="Optional JSON string list outside the repository; values are never printed")
    args = parser.parse_args()
    root = args.root.resolve()
    private = []
    if args.private_values:
        source = args.private_values.resolve()
        if source.is_relative_to(root):
            parser.error("private values must remain outside the repository")
        private = json.loads(source.read_text())
        if not isinstance(private, list) or not all(isinstance(v, str) for v in private):
            parser.error("private values must be a JSON string list")
    failures = []
    count = 0

    def check(label, data, scan_endpoint_literals=True):
        nonlocal count
        count += 1
        for rule in problems(data, private, scan_endpoint_literals):
            failures.append({"source": label, "rule": rule})

    names = set(git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard").decode().split("\0"))
    for name in sorted(names - {""}):
        path = root / name
        if path.is_file() and not path.is_symlink():
            check(name, path.read_bytes())
    if args.git_objects:
        objects = git(root, "rev-list", "--objects", "--all").splitlines()
        oids = b"\n".join(line.split(b" ", 1)[0] for line in objects if line)
        if oids:
            proc = subprocess.Popen(
                ["git", "-C", str(root), "cat-file", "--batch"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
            )
            batch_out, _ = proc.communicate(oids + b"\n")
            if proc.returncode != 0:
                raise subprocess.CalledProcessError(proc.returncode, "git cat-file --batch")
            pos = 0
            total_len = len(batch_out)
            while pos < total_len:
                nl = batch_out.index(b"\n", pos)
                header = batch_out[pos:nl].decode()
                parts = header.split(" ")
                oid = parts[0]
                if len(parts) < 3 or parts[1] == "missing":
                    pos = nl + 1
                    continue
                kind = parts[1]
                size = int(parts[2])
                data = batch_out[nl + 1 : nl + 1 + size]
                pos = nl + 1 + size + 1
                if kind == "blob":
                    check("git:" + oid, data)
                elif kind in ("commit", "tag"):
                    check("git:" + oid, data)
                    for email in re.findall(rb"^(?:author|committer|tagger) .* <([^>]+)>", data, re.M):
                        if not email.endswith(b"@users.noreply.github.com"):
                            failures.append({"source": "git:" + oid, "rule": "commit_identity"})
    for directory in args.payload:
        if not directory.is_dir():
            raise ValueError("invalid payload directory")
        for path in sorted(directory.resolve().rglob("*")):
            if path.is_file() and not path.is_symlink():
                generated_frontend = path.suffix.lower() in {".js", ".css"}
                check(
                    "payload:" + str(path.relative_to(directory.resolve())),
                    path.read_bytes(),
                    scan_endpoint_literals=not generated_frontend,
                )
    print(json.dumps({"checked": count, "failures": failures}, ensure_ascii=False))
    return bool(failures)


def main():
    try:
        return scan()
    except (OSError, ValueError, subprocess.SubprocessError):
        # Inputs can contain private paths and secrets. Never print exception
        # messages, subprocess stderr or a traceback, and never report success.
        print("privacy check failed: unable to read scan inputs", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
