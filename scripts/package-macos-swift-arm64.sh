#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
cd "${ROOT}"

die() {
  echo "package-macos-swift-arm64.sh: $*" >&2
  exit 1
}

OS="$(uname -s)"
ARCH="$(uname -m)"
if [[ "${OS}" != "Darwin" || "${ARCH}" != "arm64" ]]; then
  die "must run on macOS arm64, got ${OS}/${ARCH}"
fi

python3 "${ROOT}/scripts/privacy-check.py"

VERSION="$(python3 -c 'import pathlib,tomllib; print(tomllib.loads(pathlib.Path("Cargo.toml").read_text())["workspace"]["package"]["version"])')"
[[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "invalid Cargo workspace version: ${VERSION}"

BUNDLE_ID="${AGENTHUB_BUNDLE_ID:-com.lich13.agenthub}"
CODESIGN_ID="${MACOS_CODESIGN_IDENTITY:--}"
DIST="${DIST_DIR:-${ROOT}/dist}"
CARGO_TARGET="${CARGO_TARGET_DIR:-${ROOT}/target}"
ICON_SRC="${ROOT}/macos/Resources/icon.icns"

[[ -f "${ICON_SRC}" ]] || die "missing app icon at ${ICON_SRC}"

TMP="$(mktemp -d)"
cleanup() {
  [[ -n "${TMP}" && -d "${TMP}" ]] && rm -r -- "${TMP}"
  return 0
}
trap cleanup EXIT

export MACOSX_DEPLOYMENT_TARGET="12.0"

# Remap local $HOME out of both Rust and Swift release binaries so zero personal paths remain in Mach-O payloads
REMAP_RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${HOME}=/Users/example --remap-path-prefix=${ROOT}=/Users/example/AgentHub"

echo "==> Building agenthub-webd and libagenthub_ffi.a (aarch64-apple-darwin, macOS 12.0)..."
RUSTFLAGS="${REMAP_RUSTFLAGS}" cargo build \
  --package agenthub-webd \
  --package agenthub-ffi \
  --target aarch64-apple-darwin \
  --release \
  --locked

HELPER_BIN="${CARGO_TARGET}/aarch64-apple-darwin/release/agenthub-webd"
FFI_LIB_DIR="${CARGO_TARGET}/aarch64-apple-darwin/release"
[[ -x "${HELPER_BIN}" ]] || die "missing built helper binary at ${HELPER_BIN}"
[[ -f "${FFI_LIB_DIR}/libagenthub_ffi.a" ]] || die "missing built static archive at ${FFI_LIB_DIR}/libagenthub_ffi.a"

echo "==> Building native Swift 6 AgentHub executable (Release WMO, serialized debugging disabled)..."
AGENTHUB_FFI_LIB_DIR="${FFI_LIB_DIR}" swift build \
  --package-path "${ROOT}/macos" \
  -c release \
  -Xswiftc -O \
  -Xswiftc -whole-module-optimization \
  -Xswiftc -no-serialize-debugging-options \
  -Xswiftc -file-prefix-map -Xswiftc "${HOME}=/Users/example" \
  -Xswiftc -file-prefix-map -Xswiftc "${ROOT}=/Users/example/AgentHub"

SWIFT_BIN_DIR="$(AGENTHUB_FFI_LIB_DIR="${FFI_LIB_DIR}" swift build --package-path "${ROOT}/macos" -c release --show-bin-path)"
SWIFT_BIN="${SWIFT_BIN_DIR}/agenthub"
[[ -x "${SWIFT_BIN}" ]] || die "missing built Swift executable at ${SWIFT_BIN}"

# Strip local symbol table paths while keeping Mach-O valid for codesign
strip -x "${HELPER_BIN}" -o "${TMP}/agenthub-webd.stripped"
strip -x "${SWIFT_BIN}" -o "${TMP}/agenthub.stripped"

ARCHIVE_STAGE="${TMP}/archive-stage"
APP_BUNDLE="${ARCHIVE_STAGE}/AgentHub.app"
mkdir -p "${APP_BUNDLE}/Contents/MacOS" "${APP_BUNDLE}/Contents/Resources"

cp "${TMP}/agenthub.stripped" "${APP_BUNDLE}/Contents/MacOS/agenthub"
chmod 0755 "${APP_BUNDLE}/Contents/MacOS/agenthub"

cp "${TMP}/agenthub-webd.stripped" "${APP_BUNDLE}/Contents/Resources/agenthub-webd"
chmod 0755 "${APP_BUNDLE}/Contents/Resources/agenthub-webd"

cp "${ICON_SRC}" "${APP_BUNDLE}/Contents/Resources/icon.icns"
chmod 0644 "${APP_BUNDLE}/Contents/Resources/icon.icns"

cat > "${APP_BUNDLE}/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>AgentHub</string>
  <key>CFBundleExecutable</key>
  <string>agenthub</string>
  <key>CFBundleIconFile</key>
  <string>icon</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>AgentHub</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
EOF

if [[ "${BUNDLE_ID}" != "com.lich13.agenthub" ]]; then
  # Swift expands this on the user's Mac when opening the preview bundle.
  # shellcheck disable=SC2088
  preview_root='~/Library/Application Support/AgentHubPreview'
  plutil -insert AgentHubIsolatedDataRoot -string "${AGENTHUB_ISOLATED_DATA_DIR:-$preview_root}" "${APP_BUNDLE}/Contents/Info.plist"
fi
plutil -lint "${APP_BUNDLE}/Contents/Info.plist" >/dev/null

echo "==> Signing AgentHub.app with identity '${CODESIGN_ID}'..."
if [[ "${CODESIGN_ID}" == "-" ]]; then
  /usr/bin/codesign --force --sign "-" "${APP_BUNDLE}/Contents/Resources/agenthub-webd"
  /usr/bin/codesign --force --sign "-" "${APP_BUNDLE}"
else
  /usr/bin/codesign --force --options runtime --timestamp --sign "${CODESIGN_ID}" "${APP_BUNDLE}/Contents/Resources/agenthub-webd"
  /usr/bin/codesign --force --options runtime --timestamp --sign "${CODESIGN_ID}" "${APP_BUNDLE}"
fi

/usr/bin/codesign --verify --deep --strict "${APP_BUNDLE}"

HELPER_KIND="$(file "${APP_BUNDLE}/Contents/Resources/agenthub-webd")"
[[ "${HELPER_KIND}" == *"Mach-O 64-bit executable arm64"* ]] || die "unexpected helper binary type: ${HELPER_KIND}"
MAIN_KIND="$(file "${APP_BUNDLE}/Contents/MacOS/agenthub")"
[[ "${MAIN_KIND}" == *"Mach-O 64-bit executable arm64"* ]] || die "unexpected main binary type: ${MAIN_KIND}"
[[ ! -e "${APP_BUNDLE}/Contents/Resources/webui" ]] || die "retired webui directory must not exist in bundle"

echo "==> Running payload privacy gate on AgentHub.app..."
python3 "${ROOT}/scripts/privacy-check.py" --payload "${APP_BUNDLE}"

mkdir -p "${DIST}/bundle"
python3 - "${DIST}/bundle/AgentHub.app" <<'PY'
from pathlib import Path
import shutil, sys
target = Path(sys.argv[1]).resolve()
if target.name == "AgentHub.app" and target.parent.name == "bundle" and target.exists():
    shutil.rmtree(target)
PY
cp -R "${APP_BUNDLE}" "${DIST}/bundle/AgentHub.app"

TAR_NAME="agenthub-darwin-arm64.tar.gz"
TAR_PATH="${DIST}/${TAR_NAME}"
rm -f "${TAR_PATH}" "${TAR_PATH}.sha256" "${TAR_PATH}.sig"

# Exact top-level AgentHub.app without leading ./ for Tauri updater skip(1) compatibility
COPYFILE_DISABLE=1 tar -C "${ARCHIVE_STAGE}" -czf "${TAR_PATH}" AgentHub.app

FIRST_TAR_ENTRY="$(tar -tzf "${TAR_PATH}" | head -n 1)"
[[ "${FIRST_TAR_ENTRY}" == "AgentHub.app/" || "${FIRST_TAR_ENTRY}" == "AgentHub.app" ]] || \
  die "updater tar.gz top-level entry must be AgentHub.app without ./ prefix, got: ${FIRST_TAR_ENTRY}"
if tar -tzf "${TAR_PATH}" | grep -Eq '^\./'; then
  die "updater tar.gz must not contain ./ prefixed entries"
fi

DMG_NAME="AgentHub-${VERSION}-darwin-arm64.dmg"
DMG_PATH="${DIST}/${DMG_NAME}"
DMG_STAGE="${TMP}/dmg-stage"
mkdir -p "${DMG_STAGE}"
cp -R "${APP_BUNDLE}" "${DMG_STAGE}/AgentHub.app"
ln -s /Applications "${DMG_STAGE}/Applications"

rm -f "${DMG_PATH}" "${DMG_PATH}.sha256"
hdiutil create \
  -volname "AgentHub" \
  -srcfolder "${DMG_STAGE}" \
  -ov \
  -format UDZO \
  "${DMG_PATH}" >/dev/null

(
  cd "${DIST}"
  shasum -a 256 "${DMG_NAME}" > "${DMG_NAME}.sha256"
  shasum -a 256 "${TAR_NAME}" > "${TAR_NAME}.sha256"
)

if [[ -n "${AGENTHUB_SIGNING_PRIVATE_KEY:-}" || -n "${AGENTHUB_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
  echo "==> Signing updater archive with sign-minisign-archive.py..."
  python3 "${ROOT}/scripts/sign-minisign-archive.py" \
    --archive "${TAR_PATH}" \
    --version "${VERSION}" \
    --public-key "${SIGNING_PUBLIC_KEY_OVERRIDE:-${ROOT}/deploy/agenthub-webd/updater.pub}"
fi

echo "==> Packaged native Swift 6 AgentHub ${VERSION}:"
echo "  App: ${DIST}/bundle/AgentHub.app"
echo "  DMG: ${DMG_PATH}"
echo "  TAR: ${TAR_PATH}"
