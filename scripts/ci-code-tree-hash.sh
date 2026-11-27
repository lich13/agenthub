#!/usr/bin/env bash
set -Eeuo pipefail

REPO="${1:-.}"

python3 - "${REPO}" <<'PY'
import hashlib
import subprocess
import sys

repo = sys.argv[1]
raw = subprocess.check_output(
    ["git", "-C", repo, "ls-tree", "-z", "-r", "--full-tree", "HEAD"]
)

digest = hashlib.sha256()
digest.update(b"agenthub-code-tree-v1\0")

for entry in raw.split(b"\0"):
    if not entry:
        continue
    meta, sep, path_bytes = entry.partition(b"\t")
    if not sep:
        raise SystemExit("malformed git ls-tree entry")
    path = path_bytes.decode("utf-8", errors="surrogateescape")
    is_root_markdown = "/" not in path and path.endswith(".md")
    is_docs_markdown = path.startswith("docs/") and path.endswith(".md")
    if is_root_markdown or is_docs_markdown:
        continue
    digest.update(meta)
    digest.update(b"\t")
    digest.update(path_bytes)
    digest.update(b"\0")

print(digest.hexdigest())
PY
