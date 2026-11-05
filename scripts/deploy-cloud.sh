#!/usr/bin/env bash
# Validated deployment arguments intentionally expand on the client before SSH.
# shellcheck disable=SC2029
set -Eeuo pipefail

HOST="${1:?Usage: deploy-cloud.sh HOST [ARCHIVE]}"
ARCHIVE="${2:-dist/agenthub-webd-linux-x86_64.tar.gz}"
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
ssh "${HOST}" "sudo -n bash '${REMOTE_STAGE}/agenthub-webd/install.sh' --archive '${REMOTE_STAGE}/release.tar.gz'"
ssh "${HOST}" "sudo -n systemctl is-active --quiet agenthub-webd && test \"\$(sudo -n /usr/local/bin/agenthub-webd --version)\" = \"agenthub-webd ${EXPECTED_VERSION}\" && sudo -n /usr/local/bin/agenthub-webd health >/dev/null"

ssh "${HOST}" "rm -r -- '${REMOTE_STAGE}'"
rm -r -- "${STAGE}"
trap - ERR
