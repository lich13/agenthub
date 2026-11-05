#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
cd "${ROOT}"

required=(
  deploy/agenthub-webd/install.sh
  deploy/agenthub-webd/update.sh
  deploy/agenthub-webd/rollback.sh
  scripts/package-darwin-arm64.sh
  scripts/package-webd-linux-x86_64.sh
  scripts/package-linux.sh
  scripts/deploy-cloud.sh
  contracts/agenthub-contract.json
  contracts/agenthub-contract.schema.json
  docs/ARCHITECTURE.md
  docs/cloud-deploy-runbook.md
  docs/progress/MASTER.md
)
for path in "${required[@]}"; do
  [[ -f "${ROOT}/${path}" ]] || { echo "missing required file: ${path}" >&2; exit 1; }
done
[[ ! -e "${ROOT}/scripts/package-linux-tauri-x86_64.sh" ]] || { echo "Linux Tauri package script must be retired" >&2; exit 1; }
[[ ! -e "${ROOT}/scripts/smoke-linux-tauri.py" ]] || { echo "Linux Tauri smoke must be retired" >&2; exit 1; }

python3 - "${ROOT}" <<'PY'
from pathlib import Path
import json, re, subprocess, sys
root = Path(sys.argv[1])
expected_docs = {
    "README.md", "AGENTS.md", "DESIGN.md",
    "docs/ARCHITECTURE.md", "docs/cloud-deploy-runbook.md", "docs/progress/MASTER.md",
}
tracked = set(subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "*.md"], cwd=root, text=True).splitlines())
actual = {path for path in tracked if (root / path).is_file()}
if actual != expected_docs:
    raise SystemExit(f"Markdown set mismatch: missing={sorted(expected_docs-actual)}, extra={sorted(actual-expected_docs)}")

version = json.loads((root / "package.json").read_text())["version"]
if not re.fullmatch(r"\d+\.\d+\.\d+", version):
    raise SystemExit("package version is not semantic versioning")
for name in ["webui/package.json", "src-tauri/tauri.conf.json"]:
    if json.loads((root / name).read_text())["version"] != version:
        raise SystemExit(f"{name} version mismatch")
if f'version = "{version}"' not in (root / "Cargo.toml").read_text():
    raise SystemExit("Cargo workspace version mismatch")
config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
unit = (root / "deploy/agenthub-webd/systemd.service").read_text()
for boundary in ["Environment=GROK_LEADER_SOCKET=/var/lib/agenthub-webd/grok-leader.sock", "ProtectHome=read-only", "ProtectSystem=full", "PrivateTmp=true"]:
    if boundary not in unit:
        raise SystemExit("native Grok runtime isolation boundary missing")
write_paths = next(line for line in unit.splitlines() if line.startswith("ReadWritePaths="))
read_paths = next(line for line in unit.splitlines() if line.startswith("ReadOnlyPaths="))
for provider in (".grok", ".claude"):
    if f"-/root/{provider} " not in write_paths:
        raise SystemExit("provider data root must support atomic shared-index replacement")
import importlib.util
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("provider_paths", root / "deploy/agenthub-webd/provider-session-write-paths.py")
provider_paths = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provider_paths)
for key, provider in (("GROK_HOME", ".grok"), ("CLAUDE_CONFIG_DIR", ".claude")):
    for name in provider_paths.PROTECTED_FILES[key]:
        if f"-/root/{provider}/{name}" not in read_paths:
            raise SystemExit("provider settings or credential protection missing")
if provider_paths.custom_session_roots({"CLAUDE_CONFIG_DIR": "/srv/example-claude"}) != [Path("/srv/example-claude")]:
    raise SystemExit("custom Claude data-root whitelist missing")
if provider_paths.render({"CLAUDE_CONFIG_DIR": "/root/.claude"}):
    raise SystemExit("default Claude root must use optional unit entry")
if ".pi" in unit or provider_paths.custom_session_roots({"PI_CODING_AGENT_DIR": "/srv/fixture-pi", "PI_CODING_AGENT_SESSION_DIR": "/srv/fixture-pi-sessions"}):
    raise SystemExit("retired provider must not gain writable session paths")
for name in ["crates/agenthub-core/src/pi.rs", "crates/agenthub-webd/src/api/pi.rs", "src-tauri/src/commands/pi.rs", "webui/src/components/pi/PiWorkspace.tsx"]:
    if (root / name).exists():
        raise SystemExit("retired provider implementation remains")
if config["bundle"]["targets"] != ["dmg", "app"]:
    raise SystemExit(f"unexpected Tauri bundle targets: {config['bundle']['targets']}")

ci = (root / ".github/workflows/ci.yml").read_text()
release = (root / ".github/workflows/release.yml").read_text()
if "linux-tauri:" in ci or "linux-tauri" in release:
    raise SystemExit("Linux Tauri job remains in CI/Release")
for stale in ["AppImage", ".deb", ".rpm", '"linux-x86_64"', "xvfb-run", "package-linux-tauri-x86_64.sh"]:
    if stale in release:
        raise SystemExit(f"Release retains retired marker: {stale}")
for marker in ["webd-linux:", "macos:", "agenthub-webd-linux-x86_64.tar.gz", "darwin-aarch64", "expected eight release assets"]:
    if marker not in release:
        raise SystemExit(f"Release missing marker: {marker}")

package = (root / "scripts/package-webd-linux-x86_64.sh").read_text()
if "--dir" in package or "SKIP_WEBUI" in package:
    raise SystemExit("Linux packaging must not build front-end resources")
if (root / "deploy/agenthub-webd/web-update.sh").exists():
    raise SystemExit("retired web updater remains")
update = (root / "deploy/agenthub-webd/update.sh").read_text()
for marker in ["systemd-run", "ProtectSystem=off", "ProtectHome=off", "ReadWritePaths=/usr/local/bin", "--no-enable", "systemctl restart agenthub-webd"]:
    if marker not in update:
        raise SystemExit(f"server updater missing isolated restart marker: {marker}")


contract = json.loads((root / "contracts/agenthub-contract.json").read_text())
if "pub const API_VERSION: u32 = 2;" not in (root / "crates/agenthub-core/src/services/system.rs").read_text():
    raise SystemExit("preview-bound cleanup requires API protocol 2")
notes = subprocess.check_output([sys.executable, str(root / "scripts/release-notes.py"), version], text=True)
for title in ["版本概览", "兼容性与迁移", "支持平台与资产", "升级提示"]:
    if f"## {title}" not in notes:
        raise SystemExit("release notes are missing a required Chinese section")
if "## 功能调整" not in notes and "## 问题修复" not in notes:
    raise SystemExit("release notes must include at least one of 功能调整 or 问题修复")
unregistered = subprocess.run([sys.executable, str(root / "scripts/release-notes.py"), "9.9.9"], capture_output=True, text=True)
if unregistered.returncode == 0:
    raise SystemExit("release-notes.py must reject unregistered >= 2.0.1 versions")

print(f"AgentHub {version} install/release boundary checks: ok")
PY

shellcheck deploy/agenthub-webd/bootstrap.sh deploy/agenthub-webd/install.sh deploy/agenthub-webd/update.sh deploy/agenthub-webd/rollback.sh scripts/deploy-cloud.sh
python3 scripts/test-install-safety.py
python3 scripts/test-provider-write-paths.py
python3 scripts/test-install-signatures.py
node --test scripts/test-security-audit.mjs
python3 scripts/check-workspaces.py
python3 scripts/test-check-workspaces.py
python3 scripts/check-versions.py
python3 scripts/test-check-versions.py
python3 scripts/test-privacy-check.py
python3 scripts/privacy-check.py --git-objects
