#!/usr/bin/env python3
"""Remove only AgentHub's former HTTP entry after the SSH service is healthy."""
import importlib.util
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tomllib


def retire(config, validator, transaction, nginx_root=Path('/etc/nginx')):
    spec = importlib.util.spec_from_file_location('installer', validator)
    installer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(installer)
    snippet = nginx_root / 'snippets/agenthub.conf'
    candidates = set()
    for directory in ['sites-enabled', 'sites-available', 'conf.d']:
        root = nginx_root / directory
        if root.is_dir():
            candidates.update(path.resolve() for path in root.iterdir() if path.is_file())
    main = nginx_root / 'nginx.conf'
    if main.is_file():
        candidates.add(main)
    changed = False
    pattern = re.compile(r'^\s*include\s+(?:[\"\']?)(?:' + re.escape(str(snippet)) + r'|snippets/agenthub\.conf)(?:[\"\']?)\s*;\s*(?:#.*)?$', re.M)
    for path in sorted(candidates):
        original = path.read_text()
        updated = pattern.sub('', original)
        if updated != original:
            installer.snapshot(transaction, path)
            path.write_text(updated)
            changed = True
    if snippet.exists():
        installer.snapshot(transaction, snippet)
        snippet.unlink()
        changed = True
    if changed:
        subprocess.run(['nginx', '-t'], check=True)
        subprocess.run(['systemctl', 'reload', 'nginx'], check=True)
    db_path = Path(tomllib.loads(Path(config).read_text())['paths']['db_path'])
    if db_path.exists():
        with sqlite3.connect(db_path) as db:
            db.execute("DELETE FROM settings WHERE key='admin_api_key_sha256'")


if __name__ == '__main__':
    retire(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
