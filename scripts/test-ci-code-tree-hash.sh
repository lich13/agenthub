#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
HASH_SCRIPT="${ROOT}/scripts/ci-code-tree-hash.sh"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agenthub-ci-tree-hash.XXXXXX")"
cleanup() {
  if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
    python3 - "${TMP_DIR}" <<'PY'
import pathlib
import shutil
import sys

target = pathlib.Path(sys.argv[1]).resolve()
shutil.rmtree(target)
PY
  fi
}
trap cleanup EXIT

REPO="${TMP_DIR}/repo"
mkdir -p "${REPO}"
git -C "${REPO}" init -q
git -C "${REPO}" config user.name "fixture"
git -C "${REPO}" config user.email "fixture@users.noreply.github.com"

mkdir -p "${REPO}/crates/demo/src" "${REPO}/docs/progress"
printf 'fn main() {}\n' > "${REPO}/crates/demo/src/lib.rs"
printf '# Demo\n' > "${REPO}/README.md"
printf '# Guide\n' > "${REPO}/AGENTS.md"
printf '# Arch\n' > "${REPO}/docs/ARCHITECTURE.md"
printf '# Master\n' > "${REPO}/docs/progress/MASTER.md"
git -C "${REPO}" add .
git -C "${REPO}" commit -q -m "initial"

base_hash="$("${HASH_SCRIPT}" "${REPO}")"
[[ "${base_hash}" =~ ^[0-9a-f]{64}$ ]] || {
  echo "expected 64-char sha256 hex, got: ${base_hash}" >&2
  exit 1
}

# 1. Root *.md and docs/**/*.md edits/additions/removals must NOT change the hash.
printf '# Updated Demo\n' > "${REPO}/README.md"
printf '# Updated Arch\n' > "${REPO}/docs/ARCHITECTURE.md"
printf '# Updated Master\n' > "${REPO}/docs/progress/MASTER.md"
printf '# Extra Doc\n' > "${REPO}/docs/extra-notes.md"
git -C "${REPO}" rm -q AGENTS.md
git -C "${REPO}" add -A
git -C "${REPO}" commit -q -m "docs only"

docs_hash="$("${HASH_SCRIPT}" "${REPO}")"
if [[ "${docs_hash}" != "${base_hash}" ]]; then
  echo "docs-only commit changed code tree hash: ${base_hash} -> ${docs_hash}" >&2
  exit 1
fi

# 2. Modifying code must change the hash.
printf 'pub fn answer() -> u32 { 42 }\n' > "${REPO}/crates/demo/src/lib.rs"
git -C "${REPO}" add -A
git -C "${REPO}" commit -q -m "modify rust code"

code_hash="$("${HASH_SCRIPT}" "${REPO}")"
if [[ "${code_hash}" == "${docs_hash}" ]]; then
  echo "code modification did not change hash" >&2
  exit 1
fi

# 3. Renaming a .rs file into docs/*.md removes code and must change the hash.
git -C "${REPO}" mv crates/demo/src/lib.rs docs/moved-from-rs.md
git -C "${REPO}" commit -q -m "rename rs into docs md"

rename_hash="$("${HASH_SCRIPT}" "${REPO}")"
if [[ "${rename_hash}" == "${code_hash}" ]]; then
  echo "renaming .rs to docs/*.md did not change hash" >&2
  exit 1
fi

# 4. Non-Markdown file under docs/ must change the hash.
printf '{"schema": 1}\n' > "${REPO}/docs/fixture.json"
git -C "${REPO}" add docs/fixture.json
git -C "${REPO}" commit -q -m "non-md under docs"

docs_non_md_hash="$("${HASH_SCRIPT}" "${REPO}")"
if [[ "${docs_non_md_hash}" == "${rename_hash}" ]]; then
  echo "non-Markdown file under docs/ did not change hash" >&2
  exit 1
fi

# 5. Subdirectory README.md (outside root and docs/) must change the hash.
printf '# Crate README\n' > "${REPO}/crates/demo/README.md"
git -C "${REPO}" add crates/demo/README.md
git -C "${REPO}" commit -q -m "subdir readme"

subdir_md_hash="$("${HASH_SCRIPT}" "${REPO}")"
if [[ "${subdir_md_hash}" == "${docs_non_md_hash}" ]]; then
  echo "subdirectory README.md did not change hash" >&2
  exit 1
fi

echo "ci-code-tree-hash checks: ok"
