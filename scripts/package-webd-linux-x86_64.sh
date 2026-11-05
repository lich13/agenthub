#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
DIST="${ROOT}/dist"
ASSET="agenthub-webd-linux-x86_64.tar.gz"
ARCHIVE_ROOT="agenthub-webd-linux-x86_64"
OS="$(uname -s)"
ARCH="$(uname -m)"
CHECK_ONLY=0

for arg in "$@"; do
  case "${arg}" in
    --check) CHECK_ONLY=1 ;;
    *) echo "unknown argument: ${arg}" >&2; exit 2 ;;
  esac
done

die() { echo "package-webd-linux-x86_64.sh: $*" >&2; exit 1; }

if [[ "${ALLOW_HOST_MISMATCH:-0}" != "1" ]]; then
  if [[ "${OS}" != "Linux" || "${ARCH}" != "x86_64" ]]; then
    die "must run on Linux x86_64, got ${OS}/${ARCH}; use GitHub Actions or ALLOW_HOST_MISMATCH=1 for local smoke archives"
  fi
fi

mkdir -p "${DIST}"

cargo build --release --package agenthub-webd --locked

TMP="$(mktemp -d)"
trap '[[ -n "${TMP}" && -d "${TMP}" ]] && rm -r -- "${TMP}"' EXIT
mkdir -p "${TMP}/${ARCHIVE_ROOT}/bin" "${TMP}/${ARCHIVE_ROOT}/deploy"
cp "${CARGO_TARGET_DIR:-${ROOT}/target}/release/agenthub-webd" "${TMP}/${ARCHIVE_ROOT}/bin/"
cp -a "${ROOT}/deploy/agenthub-webd/." "${TMP}/${ARCHIVE_ROOT}/deploy/"
cp "${ROOT}/README.md" "${ROOT}/DESIGN.md" "${TMP}/${ARCHIVE_ROOT}/"
chmod 0755 "${TMP}/${ARCHIVE_ROOT}/bin/agenthub-webd"
chmod 0755 "${TMP}/${ARCHIVE_ROOT}/deploy/"*.sh

"${TMP}/${ARCHIVE_ROOT}/deploy/install.sh" --check
"${TMP}/${ARCHIVE_ROOT}/bin/agenthub-webd" --version | grep -Eq '^agenthub-webd [0-9]+\.[0-9]+\.[0-9]+'

python3 "${ROOT}/scripts/privacy-check.py" --payload "${TMP}/${ARCHIVE_ROOT}"

tar -C "${TMP}" -czf "${DIST}/${ASSET}" "${ARCHIVE_ROOT}"
(
  cd "${DIST}"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${ASSET}" > "${ASSET}.sha256"
  else
    sha256sum "${ASSET}" > "${ASSET}.sha256"
  fi
)

if [[ "${CHECK_ONLY}" -eq 1 ]]; then
  tar -tzf "${DIST}/${ASSET}" | grep -qx "${ARCHIVE_ROOT}/bin/agenthub-webd"
  if tar -tzf "${DIST}/${ASSET}" | grep -Eq "(/webui/|\.html$)"; then die "server archive contains retired web assets"; fi
  tar -tzf "${DIST}/${ASSET}" | grep -qx "${ARCHIVE_ROOT}/deploy/install.sh"
  tar -tzf "${DIST}/${ASSET}" | grep -qx "${ARCHIVE_ROOT}/deploy/provider-session-write-paths.py"
fi

TAURI_CLI="${TAURI_CLI:-${ROOT}/webui/node_modules/.bin/tauri}"
[[ -x "${TAURI_CLI}" && -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] || die "release signing tool and key are required"
VERSION="$(python3 -c 'import pathlib,tomllib; print(tomllib.loads(pathlib.Path("Cargo.toml").read_text())["workspace"]["package"]["version"])')"
"${TAURI_CLI}" signer sign --app-version "${VERSION}" "${DIST}/${ASSET}" >/dev/null
python3 "${ROOT}/deploy/agenthub-webd/install-safety.py" verify "${DIST}/${ASSET}" "${DIST}/${ASSET}.sha256" "${DIST}/${ASSET}.sig" "${ROOT}/deploy/agenthub-webd/updater.pub" "${VERSION}" >/dev/null

echo "${DIST}/${ASSET}"
