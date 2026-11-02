#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
echo "scripts/package-linux.sh is deprecated; building agenthub-webd Linux server tarball." >&2
exec "${ROOT}/scripts/package-webd-linux-x86_64.sh" "$@"
