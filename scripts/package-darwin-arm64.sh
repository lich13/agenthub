#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
cd "${ROOT}"
python3 "${ROOT}/scripts/privacy-check.py"

DIST="${ROOT}/dist"
TAURI_DIR="${TAURI_DIR:-${ROOT}/src-tauri}"
WEBUI_DIR="${WEBUI_DIR:-${ROOT}/webui}"
TAURI_TARGET_DIR="${TAURI_TARGET_DIR:-${TAURI_DIR}/target}"
TAURI_CLI="${TAURI_CLI:-${WEBUI_DIR}/node_modules/.bin/tauri}"
TMP=""
HELPER_RESOURCE=""
HELPER_RESOURCE_BACKUP=""
HELPER_RESOURCE_HAD_ORIGINAL=0
TEMP_TAURI_CONFIG=""

restore_helper_resource() {
  if [[ -n "${HELPER_RESOURCE}" && -n "${HELPER_RESOURCE_BACKUP}" && -f "${HELPER_RESOURCE_BACKUP}" ]]; then
    if [[ "${HELPER_RESOURCE_HAD_ORIGINAL}" == "1" ]]; then
      cp -p "${HELPER_RESOURCE_BACKUP}" "${HELPER_RESOURCE}"
    else
      rm -f "${HELPER_RESOURCE}"
    fi
  fi
}

cleanup() {
  [[ -n "${TMP}" && -d "${TMP}" ]] && rm -r -- "${TMP}"
  restore_helper_resource
  [[ -n "${HELPER_RESOURCE_BACKUP}" && -f "${HELPER_RESOURCE_BACKUP}" ]] && rm -f "${HELPER_RESOURCE_BACKUP}"
  [[ -n "${TEMP_TAURI_CONFIG}" && -f "${TEMP_TAURI_CONFIG}" ]] && rm -f "${TEMP_TAURI_CONFIG}"
  return 0
}

trap cleanup EXIT

cargo_package_version() {
  cargo pkgid --package agenthub-webd --manifest-path "${ROOT}/Cargo.toml" |
    awk -F# '{print $NF}'
}

die() {
  echo "package-darwin-arm64.sh: $*" >&2
  exit 1
}

find_tauri_artifact() {
  local pattern="$1"
  local root
  local found=""

  for root in \
    "${TAURI_TARGET_DIR}/release/bundle" \
    "${TAURI_TARGET_DIR}/aarch64-apple-darwin/release/bundle"
  do
    [[ -d "${root}" ]] || continue
    found="$(find "${root}" -name "${pattern}" -print 2>/dev/null | sort | tail -n 1)"
    [[ -n "${found}" ]] && break
  done

  [[ -n "${found}" ]] || die "missing Tauri artifact matching ${pattern}"
  printf '%s\n' "${found}"
}

find_tauri_signature_artifact() {
  local archive_path="${1:-}"
  local found=""

  if [[ -n "${archive_path}" && -f "${archive_path}.sig" ]]; then
    printf '%s\n' "${archive_path}.sig"
    return 0
  fi

  found="$(find_tauri_artifact "*.app.tar.gz.sig" 2>/dev/null || true)"
  [[ -n "${found}" ]] || return 1
  printf '%s\n' "${found}"
}

find_optional_tauri_artifact() {
  local pattern="$1"
  local root
  local found=""

  for root in \
    "${TAURI_TARGET_DIR}/release/bundle" \
    "${TAURI_TARGET_DIR}/aarch64-apple-darwin/release/bundle"
  do
    [[ -d "${root}" ]] || continue
    found="$(find "${root}" -name "${pattern}" -print 2>/dev/null | sort | tail -n 1)"
    [[ -n "${found}" ]] && break
  done

  printf '%s\n' "${found}"
}

assert_app_only_archive() {
  local root="$1"
  local entries=()

  [[ -d "${root}/AgentHub.app" ]] || die "archive root must contain AgentHub.app"
  while IFS= read -r entry; do
    entries+=("${entry}")
  done < <(find "${root}" -mindepth 1 -maxdepth 1 -print | sort)

  if [[ "${#entries[@]}" -ne 1 || "${entries[0]}" != "${root}/AgentHub.app" ]]; then
    die "archive root must contain only AgentHub.app"
  fi
}

assert_app_bundle_resources() {
  local app_bundle="$1"
  local helper="${app_bundle}/Contents/Resources/agenthub-webd"
  local bundled_webui="${app_bundle}/Contents/Resources/webui"

  [[ -x "${helper}" ]] || die "app bundle missing executable agenthub-webd helper"
  local helper_kind
  helper_kind="$(file "${helper}")"
  [[ "${helper_kind}" == *"Mach-O 64-bit executable arm64"* ]] ||
    die "app bundle helper must be a macOS arm64 executable, got: ${helper_kind}"

  [[ ! -e "${bundled_webui}" ]] || die "app bundle contains retired duplicate LAN WebUI resources"
  if [[ "${OS}" == "Darwin" ]]; then
    codesign --verify --deep --strict "${app_bundle}"
  fi
}

assert_helper_resource_placeholder() {
  [[ -f "${HELPER_RESOURCE}" ]] || die "helper placeholder missing after packaging cleanup"
  [[ ! -x "${HELPER_RESOURCE}" ]] || die "helper placeholder must remain non-executable after packaging cleanup"
  if ! grep -q '^AGENTHUB_HELPER_PLACEHOLDER' "${HELPER_RESOURCE}"; then
    die "helper placeholder was not restored after packaging cleanup"
  fi
}

cleanup_stale_launchservices_dmg_registrations() {
  local lsregister="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
  [[ -x "${lsregister}" ]] || return 0

  "${lsregister}" -dump 2>/dev/null |
    awk '/path: *\/Volumes\/dmg\..*\/(AgentHub|AgentHub)\.app/ {print $2}' |
    sort -u |
    while IFS= read -r stale_app; do
      [[ -n "${stale_app}" ]] || continue
      "${lsregister}" -u "${stale_app}" >/dev/null 2>&1 || true
    done
}

OS="$(uname -s)"
ARCH="$(uname -m)"

if [[ "${ALLOW_HOST_MISMATCH:-0}" != "1" ]]; then
  if [[ "${OS}" != "Darwin" || "${ARCH}" != "arm64" ]]; then
    echo "package-darwin-arm64.sh must run on macOS arm64, got ${OS}/${ARCH}" >&2
    echo "Use GitHub Actions release workflow or set ALLOW_HOST_MISMATCH=1 only for local smoke archives." >&2
    exit 1
  fi
fi

VERSION="${VERSION:-$(cargo_package_version)}"
TARBALL_ASSET="agenthub-darwin-arm64.tar.gz"
SIGNATURE_ASSET="agenthub-darwin-arm64.tar.gz.sig"
DMG_ASSET="AgentHub-${VERSION}-darwin-arm64.dmg"
SIGNED_RELEASE_CONTEXT=0
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" || -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" || "${GITHUB_ACTIONS:-}" == "true" ]]; then
  SIGNED_RELEASE_CONTEXT=1
fi
if [[ "${SIGNED_RELEASE_CONTEXT}" == "1" && -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  die "release builds require TAURI_SIGNING_PRIVATE_KEY for signed updater artifacts"
fi

[[ -d "${TAURI_DIR}" ]] || die "missing Tauri project directory: ${TAURI_DIR}"
[[ -d "${WEBUI_DIR}" ]] || die "missing WebUI project directory: ${WEBUI_DIR}"

mkdir -p "${DIST}"
mkdir -p "${TAURI_DIR}/resources"
HELPER_RESOURCE="${TAURI_DIR}/resources/agenthub-webd"
HELPER_RESOURCE_BACKUP="$(mktemp)"
if [[ -f "${HELPER_RESOURCE}" ]]; then
  cp -p "${HELPER_RESOURCE}" "${HELPER_RESOURCE_BACKUP}"
  HELPER_RESOURCE_HAD_ORIGINAL=1
fi

if [[ "${SKIP_WEBUI_INSTALL:-0}" != "1" ]]; then
  pnpm --dir "${WEBUI_DIR}" install --frozen-lockfile
fi

[[ -x "${TAURI_CLI}" ]] || die "missing Tauri CLI: ${TAURI_CLI}"

if [[ "${SKIP_HELPER_BUILD:-0}" != "1" ]]; then
  cargo build --release --package agenthub-webd --locked
fi

HELPER_BINARY="${CARGO_TARGET_DIR:-${ROOT}/target}/release/agenthub-webd"
[[ -x "${HELPER_BINARY}" ]] || die "missing helper binary: ${HELPER_BINARY}"
cp "${HELPER_BINARY}" "${HELPER_RESOURCE}"
chmod 755 "${HELPER_RESOURCE}"

TEMP_TAURI_CONFIG="$(mktemp)"
python3 "${ROOT}/scripts/tauri-build-config.py" \
  "${TAURI_DIR}/tauri.conf.json" "${TEMP_TAURI_CONFIG}" "${WEBUI_DIR}" \
  "${SIGNED_RELEASE_CONTEXT}" "${SKIP_WEBUI_BUILD:-0}"
TAURI_BUILD_CONFIG="${TEMP_TAURI_CONFIG}"

if [[ "${SKIP_TAURI_BUILD:-0}" != "1" ]]; then
  "${TAURI_CLI}" build --config "${TAURI_BUILD_CONFIG}" --bundles app,dmg -- --locked
fi

APP_BUNDLE="$(find_tauri_artifact "AgentHub.app")"
TAURI_DMG="$(find_tauri_artifact "*.dmg")"
TAURI_UPDATER_ARCHIVE="$(find_optional_tauri_artifact "*.app.tar.gz")"
TAURI_UPDATER_SIGNATURE="$(find_tauri_signature_artifact "${TAURI_UPDATER_ARCHIVE}" 2>/dev/null || true)"

if [[ "${SIGNED_RELEASE_CONTEXT}" == "1" && -z "${TAURI_UPDATER_SIGNATURE}" ]]; then
  die "missing Tauri updater signature; set TAURI_SIGNING_PRIVATE_KEY for release builds"
fi

assert_app_bundle_resources "${APP_BUNDLE}"
python3 "${ROOT}/scripts/privacy-check.py" --payload "${APP_BUNDLE}"

TMP="$(mktemp -d)"

APP_ARCHIVE_ROOT="${TMP}/app-archive"
mkdir -p "${APP_ARCHIVE_ROOT}"
cp -a "${APP_BUNDLE}" "${APP_ARCHIVE_ROOT}/AgentHub.app"
assert_app_bundle_resources "${APP_ARCHIVE_ROOT}/AgentHub.app"

assert_app_only_archive "${APP_ARCHIVE_ROOT}"
if [[ -n "${TAURI_UPDATER_ARCHIVE}" && -n "${TAURI_UPDATER_SIGNATURE}" ]]; then
  cp "${TAURI_UPDATER_ARCHIVE}" "${DIST}/${TARBALL_ASSET}"
  cp "${TAURI_UPDATER_SIGNATURE}" "${DIST}/${SIGNATURE_ASSET}"
else
  tar -C "${APP_ARCHIVE_ROOT}" -czf "${DIST}/${TARBALL_ASSET}" AgentHub.app
fi

cp "${TAURI_DMG}" "${DIST}/${DMG_ASSET}"
cleanup_stale_launchservices_dmg_registrations

(
  cd "${DIST}"
  shasum -a 256 "${DMG_ASSET}" > "${DMG_ASSET}.sha256"
)

echo "${DIST}/${TARBALL_ASSET}"
echo "${DIST}/${DMG_ASSET}"
cleanup
assert_helper_resource_placeholder
trap - EXIT
