#!/usr/bin/env bash
set -Eeuo pipefail
stage=system
work=''
finish() {
  local status=$?
  trap - EXIT
  if [[ -n "${work}" ]]; then rm -r -- "${work}"; fi
  if [[ "${status}" -ne 0 ]]; then printf 'AGENTHUB_INSTALL_ERROR:%s\n' "${stage}"; fi
  exit "${status}"
}
trap finish EXIT
[[ "${EUID}" -eq 0 && "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]]
command -v systemctl >/dev/null
stage=dependencies
missing=()
for cmd in curl python3 minisign; do command -v "${cmd}" >/dev/null || missing+=("${cmd}"); done
if [[ "${#missing[@]}" -gt 0 ]]; then
  # shellcheck source=/dev/null
  . /etc/os-release
  [[ "${ID}" == ubuntu || "${ID}" == debian ]]
  apt-get update >&2
  DEBIAN_FRONTEND=noninteractive apt-get install -y curl ca-certificates python3 minisign >&2
fi
python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)'
work="$(mktemp -d)"
cat > "${work}/updater.pub" <<'AGENTHUB_PUBLIC_KEY'
@PUBLIC_KEY@
AGENTHUB_PUBLIC_KEY
cat > "${work}/install-safety.py" <<'AGENTHUB_VALIDATOR'
@VALIDATOR@
AGENTHUB_VALIDATOR
stage=download
asset=agenthub-webd-linux-x86_64.tar.gz
base='https://github.com/lich13/agenthub/releases/download/v@VERSION@'
for suffix in '' .sha256 .sig; do
  curl -fsSL "${base}/${asset}${suffix}" -o "${work}/${asset}${suffix}"
done
stage=signature
version="$(python3 "${work}/install-safety.py" verify "${work}/${asset}" "${work}/${asset}.sha256" "${work}/${asset}.sig" "${work}/updater.pub" '@VERSION@')"
mkdir "${work}/payload"
python3 "${work}/install-safety.py" extract "${work}/${asset}" "${work}/payload"
root="${work}/payload/agenthub-webd-linux-x86_64"
python3 "${work}/install-safety.py" validate-payload "${root}" "${version}" >&2
stage=install
bash "${root}/deploy/install.sh" --archive "${work}/${asset}" >&2
stage=health
/usr/local/bin/agenthub-webd health >/dev/null
