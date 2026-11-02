#!/usr/bin/env bash
# Validated deployment arguments intentionally expand on the client before SSH.
# shellcheck disable=SC2029
set -Eeuo pipefail

HOST="${1:?Usage: deploy-cloud.sh HOST [ARCHIVE]; set AGENTHUB_DOMAIN explicitly}"
ARCHIVE="${2:-dist/agenthub-webd-linux-x86_64.tar.gz}"
DOMAIN="${AGENTHUB_DOMAIN:?Set AGENTHUB_DOMAIN explicitly}"
PATH_PREFIX="${AGENTHUB_PATH_PREFIX:-/agenthub/}"
EXPECTED_VERSION="${AGENTHUB_EXPECTED_VERSION:-}"

if [[ ! -f "${ARCHIVE}" ]]; then
  echo "archive not found: ${ARCHIVE}" >&2
  exit 1
fi

if [[ -z "${EXPECTED_VERSION}" ]]; then
  EXPECTED_VERSION="$(python3 - <<'PY'
import pathlib
import re

text = pathlib.Path("Cargo.toml").read_text(encoding="utf-8")
match = re.search(r'(?m)^version = "([^"]+)"', text)
if not match:
    raise SystemExit("workspace version not found in Cargo.toml")
print(match.group(1))
PY
)"
fi

[[ "${HOST}" =~ ^[a-zA-Z0-9_][a-zA-Z0-9_.@:-]*$ ]] || { echo "invalid HOST" >&2; exit 1; }
[[ "${DOMAIN}" =~ ^[a-zA-Z0-9][a-zA-Z0-9.-]*[a-zA-Z0-9]$ ]] || { echo "invalid AGENTHUB_DOMAIN" >&2; exit 1; }
[[ "${PATH_PREFIX}" =~ ^/[a-zA-Z0-9_/-]+/$ ]] || { echo "invalid path prefix" >&2; exit 1; }
[[ "${EXPECTED_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid version" >&2; exit 1; }
# Bootstrap from the trusted checkout, including on hosts whose old updater did not verify signatures.
python3 deploy/agenthub-webd/install-safety.py verify "${ARCHIVE}" "${ARCHIVE}.sha256" "${ARCHIVE}.sig" deploy/agenthub-webd/updater.pub "${EXPECTED_VERSION}" >/dev/null
ssh "${HOST}" 'command -v minisign >/dev/null' || { echo "install minisign from the host package manager before deployment" >&2; exit 1; }
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/agenthub-deploy.XXXXXXXX")"
REMOTE_STAGE="$(ssh "${HOST}" 'mktemp -d /tmp/agenthub-deploy.XXXXXXXX')"
[[ "${REMOTE_STAGE}" =~ ^/tmp/agenthub-deploy\.[a-zA-Z0-9]+$ ]] || { echo "invalid remote staging path" >&2; exit 1; }
trap 'echo "Deployment staging retained for diagnosis: ${STAGE} and ${REMOTE_STAGE}" >&2' ERR
scp "${ARCHIVE}" "${HOST}:${REMOTE_STAGE}/release.tar.gz"
tar -C deploy -czf "${STAGE}/deploy.tar.gz" agenthub-webd
scp "${STAGE}/deploy.tar.gz" "${HOST}:${REMOTE_STAGE}/deploy.tar.gz"
ARCHIVE_SHA="$(shasum -a 256 -- "${ARCHIVE}" | cut -d ' ' -f 1)"
DEPLOY_SHA="$(shasum -a 256 -- "${STAGE}/deploy.tar.gz" | cut -d ' ' -f 1)"
[[ "${ARCHIVE_SHA}" =~ ^[a-f0-9]{64}$ && "${DEPLOY_SHA}" =~ ^[a-f0-9]{64}$ ]] || { echo "invalid local checksum" >&2; exit 1; }
ssh "${HOST}" "cd '${REMOTE_STAGE}' && printf '%s  %s\\n' '${ARCHIVE_SHA}' release.tar.gz '${DEPLOY_SHA}' deploy.tar.gz | sha256sum -c -"
ssh "${HOST}" "tar -xzf '${REMOTE_STAGE}/deploy.tar.gz' -C '${REMOTE_STAGE}'"
ssh "${HOST}" "sudo -n bash '${REMOTE_STAGE}/agenthub-webd/install.sh' --archive '${REMOTE_STAGE}/release.tar.gz' --domain '${DOMAIN}' --path-prefix '${PATH_PREFIX}'"
ssh "${HOST}" "sudo -n systemctl is-active --quiet agenthub-webd && test \"\$(sudo -n /usr/local/bin/agenthub-webd --version)\" = \"agenthub-webd ${EXPECTED_VERSION}\" && curl -fsS http://127.0.0.1:15742/healthz >/dev/null"

PUBLIC_BASE="https://${DOMAIN%/}${PATH_PREFIX}"
PUBLIC_BASE="${PUBLIC_BASE%/}/"

expect_http_status() {
  local url="$1"
  local expected="$2"
  local status
  status="$(curl -sS -o /dev/null -w "%{http_code}" "${url}")"
  if [[ "${status}" != "${expected}" ]]; then
    echo "unexpected HTTP status for ${url}: got ${status}, expected ${expected}" >&2
    exit 1
  fi
}

expect_404_or_not_agenthub() {
  local url="$1"
  local body
  local status
  local tmp
  tmp="$(mktemp)"
  status="$(curl -sS -o "${tmp}" -w "%{http_code}" "${url}")"
  if [[ "${status}" == "404" ]]; then
    rm -- "${tmp}"
    return 0
  fi
  body="$(tr -d '\000' <"${tmp}")"
  rm -- "${tmp}"
  if grep -Eiq 'agenthub|"label"[[:space:]]*:[[:space:]]*"Probe"|"flavor"[[:space:]]*:[[:space:]]*"builtin"' <<<"${body}"; then
    echo "legacy path appears to be handled by AgentHub: ${url} returned ${status}" >&2
    exit 1
  fi
}

expect_http_status "${PUBLIC_BASE}" "404"
expect_http_status "${PUBLIC_BASE}healthz" "200"
expect_http_status "${PUBLIC_BASE}index.html" "404"
API_STATUS="$(curl -sS -o /dev/null -w "%{http_code}" -X POST -H "Content-Type: application/json" -d '{}' "${PUBLIC_BASE}api/rpc/system.capabilities")"
[[ "${API_STATUS}" == "401" ]] || { echo "unauthenticated API must reject requests" >&2; exit 1; }
expect_http_status "https://${DOMAIN%/}/codex-cloud-panel/" "404"
expect_404_or_not_agenthub "https://${DOMAIN%/}/api/sentinel/status"
expect_404_or_not_agenthub "https://${DOMAIN%/}/api/probe/status"
expect_404_or_not_agenthub "https://${DOMAIN%/}/api/v1/models"
expect_http_status "${PUBLIC_BASE}api/sentinel/status" "404"
expect_http_status "${PUBLIC_BASE}api/probe/status" "404"

ssh "${HOST}" "rm -r -- '${REMOTE_STAGE}'"
rm -r -- "${STAGE}"
trap - ERR
