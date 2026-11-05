#!/usr/bin/env bash
set -Eeuo pipefail

APP_NAME="agenthub-webd"
BIN_NAME="agenthub-webd"
SERVICE_NAME="agenthub-webd"
INSTALL_BIN="/usr/local/bin/${BIN_NAME}"
SHARE_DIR="/usr/share/${APP_NAME}"
CONFIG_DIR="/etc/${APP_NAME}"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
ENV_FILE="${CONFIG_DIR}/env"
DATA_DIR="/var/lib/${APP_NAME}"
LOG_DIR="/var/log/${APP_NAME}"
BACKUP_DIR="${DATA_DIR}/backups"
SYSTEMD_UNIT="/etc/systemd/system/${SERVICE_NAME}.service"
SYSTEMD_DROPIN_DIR="/etc/systemd/system/${SERVICE_NAME}.service.d"
PROVIDER_PATHS_DROPIN="${SYSTEMD_DROPIN_DIR}/20-provider-session-paths.conf"
UPDATE_BIN="/usr/local/bin/${APP_NAME}-update"
CODEX_PRECHECK_WRAPPER_BIN="/usr/local/bin/agenthub-codex-precheck"
CODEX_UPDATE_WRAPPER_BIN="/usr/local/bin/agenthub-codex-update"
CODEX_PRUNE_WRAPPER_BIN="/usr/local/bin/agenthub-codex-prune"

ARCHIVE_PATH=""
BINARY_PATH=""
FORCE_CONFIG=0
ENABLE_SERVICE=1
CHECK_ONLY=0

usage() {
  cat <<'USAGE'
Install AgentHub SSH service.

Usage:
  sudo install.sh --archive ./agenthub-webd-linux-x86_64.tar.gz
  sudo install.sh --binary ./target/release/agenthub-webd
  install.sh --check

Options:
  --archive PATH       Install release tarball.
  --binary PATH        Install local binary without packaged deployment tools.
  --force-config       Replace existing config.toml.
  --no-enable          Do not enable/start service.
  --check              Validate archive layout when run from extracted deploy/.
  -h, --help           Show help.
USAGE
}

log() { printf '[%s] %s\n' "${APP_NAME}" "$*"; }
die() { printf '[%s] ERROR: %s\n' "${APP_NAME}" "$*" >&2; exit 1; }

script_dir() {
  cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd -P
}

archive_root_from_deploy() {
  cd -- "$(script_dir)/.." >/dev/null 2>&1 && pwd -P
}

check_layout() {
  local root
  root="$(archive_root_from_deploy)"
  [[ -x "${root}/bin/${BIN_NAME}" ]] || die "archive missing executable bin/${BIN_NAME}"
  [[ -f "${root}/deploy/updater.pub" ]] || die "archive missing updater public key"
  [[ -f "${root}/deploy/install-safety.py" ]] || die "archive missing installer validator"
  [[ -f "${root}/deploy/install.sh" ]] || die "archive missing deploy/install.sh"
  [[ -f "${root}/deploy/update.sh" ]] || die "archive missing deploy/update.sh"
  [[ -f "${root}/deploy/systemd.service" ]] || die "archive missing deploy/systemd.service"
  [[ -f "${root}/deploy/provider-session-write-paths.py" ]] || die "archive missing deploy/provider-session-write-paths.py"
  "${root}/bin/${BIN_NAME}" --version | grep -Eq '^agenthub-webd [0-9]+\.[0-9]+\.[0-9]+'
  echo "agenthub-webd archive layout ok"
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --archive|--binary)
        [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || die "option requires a value"
        case "$1" in
      --archive) ARCHIVE_PATH="${2:-}"; shift 2 ;;
      --binary) BINARY_PATH="${2:-}"; shift 2 ;;
        esac ;;
      --force-config) FORCE_CONFIG=1; shift ;;
      --no-enable) ENABLE_SERVICE=0; shift ;;
      --check) CHECK_ONLY=1; shift ;;
      -h|--help) usage; exit 0 ;;
      *) die "unknown argument: $1" ;;
    esac
  done
  if [[ "${CHECK_ONLY}" -eq 1 ]]; then
    return
  fi
  [[ -n "${ARCHIVE_PATH}" || -n "${BINARY_PATH}" ]] || die "pass --archive or --binary"

}

require_root() {
  [[ "${EUID}" -eq 0 ]] || die "run as root"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

install_dirs() {
  install -d -m 0755 -o root -g root /usr/local/bin "${SHARE_DIR}" "${CONFIG_DIR}"
  install -d -m 0750 -o root -g root "${DATA_DIR}" "${LOG_DIR}" "${BACKUP_DIR}"
}

assert_service_version() {
  python3 - "$("$1" --version)" <<'PYVERSION'
import re, sys
match = re.fullmatch(r"agenthub-webd (\d+)\.(\d+)\.(\d+)", sys.argv[1])
if not match or tuple(map(int, match.groups())) < (2, 1, 0):
    raise SystemExit("SSH service requires v2.1.0 or newer")
PYVERSION
}

install_payload() {
  if [[ -n "${BINARY_PATH}" ]]; then
    [[ -f "${BINARY_PATH}" ]] || die "binary not found: ${BINARY_PATH}"
    assert_service_version "${BINARY_PATH}"
    replace_file "${BINARY_PATH}" "${INSTALL_BIN}" 0755
    return
  fi
  [[ -f "${ARCHIVE_PATH}" ]] || die "archive not found: ${ARCHIVE_PATH}"
  local root
  PAYLOAD_TMP="$(mktemp -d)"
  python3 "${SAFETY}" extract "${ARCHIVE_PATH}" "${PAYLOAD_TMP}"
  root="${PAYLOAD_TMP}/agenthub-webd-linux-x86_64"
  [[ -x "${root}/bin/${BIN_NAME}" ]] || die "archive missing bin/${BIN_NAME}"
  assert_service_version "${root}/bin/${BIN_NAME}"
  replace_file "${root}/bin/${BIN_NAME}" "${INSTALL_BIN}" 0755
  if [[ -d "${root}/deploy" ]]; then
    [[ ! -f "${root}/deploy/update.sh" ]] || replace_file "${root}/deploy/update.sh" "${UPDATE_BIN}" 0755
  fi
}

install_codex_wrappers() {
  local source_dir
  source_dir="$(script_dir)"
  replace_file "${source_dir}/agenthub-codex-precheck" "${CODEX_PRECHECK_WRAPPER_BIN}" 0755
  replace_file "${source_dir}/agenthub-codex-update" "${CODEX_UPDATE_WRAPPER_BIN}" 0755
  replace_file "${source_dir}/agenthub-codex-prune" "${CODEX_PRUNE_WRAPPER_BIN}" 0755
}

install_codex_home_write_paths() {
  install -d -m 0700 -o root -g root /root/.codex

  local ubuntu_owner="root"
  local ubuntu_group="root"
  if getent passwd ubuntu >/dev/null 2>&1; then
    ubuntu_owner="ubuntu"
    ubuntu_group="$(id -gn ubuntu 2>/dev/null || printf 'ubuntu')"
  fi

  [[ -d /home ]] || install -d -m 0755 -o root -g root /home
  [[ -d /home/ubuntu ]] || install -d -m 0755 -o "${ubuntu_owner}" -g "${ubuntu_group}" /home/ubuntu
  install -d -m 0700 -o "${ubuntu_owner}" -g "${ubuntu_group}" /home/ubuntu/.codex
}

install_config() {
  local source_dir
  source_dir="$(script_dir)"

  if [[ ! -f "${CONFIG_FILE}" || "${FORCE_CONFIG}" -eq 1 ]]; then
    replace_file "${source_dir}/config.example.toml" "${CONFIG_FILE}" 0640

  fi
  if [[ ! -f "${ENV_FILE}" ]]; then
    replace_file "${source_dir}/env.example" "${ENV_FILE}" 0600
  fi
  snapshot_file "${ENV_FILE}"
  snapshot_file "${CONFIG_FILE}"
  ensure_secret_key
  ensure_config_defaults
}

ensure_secret_key() {
  python3 - "${ENV_FILE}" <<'PYKEY'
from pathlib import Path
import os
import secrets
import sys
import tempfile

path = Path(sys.argv[1])
text = path.read_text()
key = "AGENTHUB_SECRET_KEY"

def entry(line):
    name, _, value = line.strip().removeprefix("export ").partition("=")
    return name.strip(), value.strip().strip('"').strip("'")

def read_key(source, name):
    if source.is_file():
        return next((value for line in source.read_text().splitlines()
                     for field, value in [entry(line)] if field == name and value), "")
    return ""

secret = read_key(path, key)
if not secret:
    secret = secrets.token_urlsafe(32)
    lines = text.splitlines(keepends=True)
    replaced = False
    updated = []
    for line in lines:
        if entry(line)[0] == key:
            if not replaced:
                updated.append(f"{key}={secret}\n")
                replaced = True
        else:
            updated.append(line)
    if not replaced:
        if updated and not updated[-1].endswith("\n"):
            updated.append("\n")
        updated.append(f"{key}={secret}\n")
    fd, pending = tempfile.mkstemp(dir=path.parent, prefix=".env-")
    try:
        with os.fdopen(fd, "w") as output:
            output.write("".join(updated))
            output.flush()
            os.fsync(output.fileno())
        os.replace(pending, path)
    finally:
        Path(pending).unlink(missing_ok=True)
path.chmod(0o600)
PYKEY
  chown root:root "${ENV_FILE}"
}

ensure_config_defaults() {
  python3 - "${CONFIG_FILE}" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
lines = text.splitlines()

def ensure_section(section, required_values):
    global lines
    start = None
    end = len(lines)
    for index, line in enumerate(lines):
        if line.strip() == f'[{section}]':
            start = index
            continue
        if start is not None and index > start and line.strip().startswith('[') and line.strip().endswith(']'):
            end = index
            break
    if start is None:
        if lines and lines[-1].strip():
            lines.append('')
        lines.append(f'[{section}]')
        start = len(lines) - 1
        end = len(lines)
    seen = set()
    for index in range(start + 1, end):
        stripped = lines[index].strip()
        if not stripped or stripped.startswith('#') or '=' not in stripped:
            continue
        key, value = [part.strip() for part in stripped.split('=', 1)]
        if key in required_values:
            seen.add(key)
    insert_at = end
    for key, value in required_values.items():
        if key not in seen:
            lines.insert(insert_at, f'{key} = {value}')
            insert_at += 1
ensure_section('codex', {'host_label': '"AgentHub"'})
ensure_section('paths', {'data_dir': '"/var/lib/agenthub-webd"', 'db_path': '"/var/lib/agenthub-webd/agenthub.sqlite"', 'log_dir': '"/var/log/agenthub-webd"'})
ensure_section('update', {'precheck_command': '"/usr/local/bin/agenthub-codex-precheck"', 'update_command': '"/usr/local/bin/agenthub-codex-update"', 'prune_command': '"/usr/local/bin/agenthub-codex-prune"', 'panel_update_command': '"/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest"', 'panel_precheck_command': '"/usr/local/bin/agenthub-webd-update --precheck"'})
ensure_section('probe', {'enabled': 'true', 'poll_seconds': '15', 'recent_limit': '50'})
ensure_section('probe.hooks', {'manage_stop_hook': 'true'})
ensure_section('probe.notifications', {'enabled': 'false', 'server_url': '"https://api.day.app"', 'group': '"AgentHub"', 'notify_completion': 'true', 'notify_reply_needed': 'true', 'notify_recoverable': 'true'})
ensure_section('probe.observability', {'hook_event_max_lines': '500', 'hook_cooldown_max_lines': '1000', 'log_max_bytes': '5242880'})
path.write_text('\n'.join(lines) + '\n')

PY
}

install_provider_session_write_paths() {
  local source_dir rendered
  source_dir="$(script_dir)"
  rendered="$(mktemp)"
  if ! python3 "${source_dir}/provider-session-write-paths.py" "${ENV_FILE}" "${CONFIG_FILE}" "${rendered}"; then
    rm -- "${rendered}"
    die "invalid custom provider session root in ${ENV_FILE}"
  fi
  if [[ -e "${PROVIDER_PATHS_DROPIN}" || -L "${PROVIDER_PATHS_DROPIN}" ]]; then
    if [[ ! -f "${PROVIDER_PATHS_DROPIN}" || -L "${PROVIDER_PATHS_DROPIN}" ]]; then
      rm -- "${rendered}"
      die "managed provider path drop-in is not a regular file: ${PROVIDER_PATHS_DROPIN}"
    fi
  fi
  if [[ -s "${rendered}" ]]; then
    install -d -m 0755 -o root -g root "${SYSTEMD_DROPIN_DIR}"
    replace_file "${rendered}" "${PROVIDER_PATHS_DROPIN}" 0644
  elif [[ -f "${PROVIDER_PATHS_DROPIN}" ]]; then
    snapshot_file "${PROVIDER_PATHS_DROPIN}"
    rm -- "${PROVIDER_PATHS_DROPIN}"
  fi
  rm -- "${rendered}"
}

install_systemd() {
  local source_dir
  source_dir="$(script_dir)"
  replace_file "${source_dir}/systemd.service" "${SYSTEMD_UNIT}" 0644
  install_provider_session_write_paths
  systemctl daemon-reload
  if [[ "${ENABLE_SERVICE}" -eq 1 ]]; then
    systemctl enable "${SERVICE_NAME}.service"
    systemctl restart "${SERVICE_NAME}.service"
  fi
}

SAFETY="$(script_dir)/install-safety.py"
TRANSACTION=""
PAYLOAD_TMP=""
INSTALL_SUCCEEDED=0
WAS_ACTIVE=0

snapshot_file() { python3 "${SAFETY}" snapshot "${TRANSACTION}" "$1"; }
replace_file() { python3 "${SAFETY}" replace "${TRANSACTION}" "$1" "$2" "$3"; }
finish_install() {
  local status=$?
  trap - EXIT
  if [[ "${INSTALL_SUCCEEDED}" -eq 0 && -n "${TRANSACTION}" ]]; then
    if python3 "${SAFETY}" rollback "${TRANSACTION}"; then
      systemctl daemon-reload || true
      if [[ "${WAS_ACTIVE}" -eq 1 ]]; then systemctl restart "${SERVICE_NAME}.service" || true; fi
    else
      log "rollback incomplete; installation backup retained"
      return 1
    fi
  fi
  [[ -z "${PAYLOAD_TMP}" ]] || rm -r -- "${PAYLOAD_TMP}"
  [[ -z "${TRANSACTION}" ]] || rm -r -- "${TRANSACTION}"
  return "${status}"
}

main() {
  parse_args "$@"
  if [[ "${CHECK_ONLY}" -eq 1 ]]; then
    check_layout
    return
  fi
  require_root
  require_command python3
  require_command minisign
  TRANSACTION="$(mktemp -d)"
  trap finish_install EXIT
  if systemctl is-active --quiet "${SERVICE_NAME}.service"; then WAS_ACTIVE=1; fi
  install_dirs
  install_payload

  install_codex_wrappers
  replace_file "${SAFETY}" "${SHARE_DIR}/install-safety.py" 0644
  replace_file "$(script_dir)/updater.pub" "${SHARE_DIR}/updater.pub" 0644
  install_config
  install_codex_home_write_paths
  install_systemd
  if [[ "${ENABLE_SERVICE}" -eq 1 ]]; then
    systemctl is-active --quiet "${SERVICE_NAME}.service"
    local ready=0
    for _ in {1..30}; do
      if "${INSTALL_BIN}" health >/dev/null 2>&1; then ready=1; break; fi
      sleep 0.2
    done
    [[ "${ready}" -eq 1 ]] || die "SSH service health check failed"

  fi
  INSTALL_SUCCEEDED=1
  log "installed ${INSTALL_BIN}"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
