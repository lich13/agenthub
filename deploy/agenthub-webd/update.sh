#!/usr/bin/env bash
set -Eeuo pipefail

REPO="${AGENTHUB_WEBD_REPO:-lich13/agenthub}"
VERSION="latest"
ASSET="agenthub-webd-linux-x86_64.tar.gz"
TMP_DIR="$(mktemp -d)"
trap '[[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]] && rm -r -- "${TMP_DIR}"' EXIT

usage() {
  cat <<'USAGE'
Update AgentHub SSH service.

Usage:
  agenthub-webd-update --repo lich13/agenthub --version latest
  agenthub-webd-update v1.2.0
  agenthub-webd-update --precheck

Options:
  --repo REPO       GitHub repo. Default: lich13/agenthub
  --version TAG     Release tag or latest. Default: latest
  --precheck        Verify installed service and SSH service health only.
  -h, --help        Show help.
USAGE
}

ORIGINAL_ARGS=("$@")
PRECHECK=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) REPO="${2:-}"; shift 2 ;;
    --version) VERSION="${2:-}"; shift 2 ;;
    --precheck) PRECHECK=1; shift ;;
    -h|--help) usage; exit 0 ;;
    --*) echo "unknown argument: $1" >&2; exit 2 ;;
    *) VERSION="$1"; shift ;;
  esac
done

[[ "${REPO}" =~ ^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$ ]] || { echo "invalid repository" >&2; exit 2; }
[[ "${VERSION}" == latest || "${VERSION}" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid version" >&2; exit 2; }

# The API service is intentionally mounted with ProtectSystem=full. Run the
# mutating install step in a short-lived root unit with its own writable host
# namespace, while keeping the long-lived webd service locked down.
if [[ "${PRECHECK}" -eq 0 && "${AGENTHUB_WEBD_UPDATE_TRANSIENT:-0}" != "1" ]]; then
  command -v systemd-run >/dev/null 2>&1 || {
    echo "systemd-run is required for a server update" >&2
    exit 1
  }
  exec systemd-run \
    --wait \
    --collect \
    --pipe \
    --unit="agenthub-webd-update-$(date +%s)-$$" \
    --property=User=root \
    --property=Group=root \
    --property=Type=exec \
    --property=ProtectSystem=off \
    --property=ProtectHome=off \
    --property=PrivateTmp=no \
    /usr/bin/env AGENTHUB_WEBD_UPDATE_TRANSIENT=1 "$0" "${ORIGINAL_ARGS[@]}"
fi

if [[ "${PRECHECK}" -eq 1 ]]; then
  test -x /usr/local/bin/agenthub-webd
  systemctl is-active --quiet agenthub-webd
  /usr/local/bin/agenthub-webd health >/dev/null
  exit 0
fi

if [[ "$(id -u)" -ne 0 ]]; then
  echo "update.sh must run as root" >&2
  exit 1
fi

if [[ "${VERSION}" == "latest" ]]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  RELEASE_TAG="${VERSION}"
  if [[ "${RELEASE_TAG}" != v* ]]; then
    RELEASE_TAG="v${RELEASE_TAG}"
  fi
  BASE_URL="https://github.com/${REPO}/releases/download/${RELEASE_TAG}"
fi

SAFETY="/usr/share/agenthub-webd/install-safety.py"
PUBLIC_KEY="/usr/share/agenthub-webd/updater.pub"
if ! command -v minisign >/dev/null 2>&1 || [[ ! -f "${PUBLIC_KEY}" || ! -f "${SAFETY}" ]]; then
  echo "installed signature verifier and public key are required" >&2
  exit 1
fi
curl -fL "${BASE_URL}/${ASSET}" -o "${TMP_DIR}/${ASSET}"
curl -fL "${BASE_URL}/${ASSET}.sha256" -o "${TMP_DIR}/${ASSET}.sha256"
curl -fL "${BASE_URL}/${ASSET}.sig" -o "${TMP_DIR}/${ASSET}.sig"
SIGNED_VERSION="$(python3 "${SAFETY}" verify "${TMP_DIR}/${ASSET}" "${TMP_DIR}/${ASSET}.sha256" "${TMP_DIR}/${ASSET}.sig" "${PUBLIC_KEY}" "${VERSION}")"
mkdir "${TMP_DIR}/payload"
python3 "${SAFETY}" extract "${TMP_DIR}/${ASSET}" "${TMP_DIR}/payload"
ROOT="${TMP_DIR}/payload/agenthub-webd-linux-x86_64"
python3 "${SAFETY}" validate-payload "${ROOT}" "${SIGNED_VERSION}"

"${ROOT}/deploy/install.sh" --archive "${TMP_DIR}/${ASSET}" --no-enable

# Let the update job finish and persist its successful result before the
# service is restarted. The restart runs outside the API service cgroup.
systemd-run \
  --collect \
  --unit="agenthub-webd-restart-$(date +%s)-$$" \
  --on-active=2s \
  /usr/bin/systemctl restart agenthub-webd
