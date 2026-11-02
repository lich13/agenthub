#!/usr/bin/env python3
"""Render native data roots and read-only provider settings for the service."""

from __future__ import annotations

import os
from pathlib import Path
import shlex
import sys
import tomllib


DEFAULT_ROOTS = {
    Path("/root/.codex"),
    Path("/home/ubuntu/.codex"),
    Path("/root/.grok"),
    Path("/root/.claude"),
    Path("/root/.dsh/sessions"),
    Path("/root/.dsh/storages"),
    Path("/root/.gemini/antigravity"),
}
DEFAULT_DSH_HOMES = {Path("/root/.dsh")}
PROTECTED_FILES = {
    "GROK_HOME": ("auth.json", "config.toml", "endpoint.env", "sandbox.toml", "trusted_folders.toml"),
    "CLAUDE_CONFIG_DIR": (".credentials.json", ".claude.json", "settings.json", "settings.local.json"),
}
FORBIDDEN_ROOTS = {
    Path("/"),
    Path("/root"),
    Path("/home"),
    Path("/etc"),
    Path("/usr"),
    Path("/var"),
}
SUPPORTED_KEYS = {
    "CODEX_HOME",
    "GROK_HOME",
    "CLAUDE_CONFIG_DIR",
    "DSH_HOME",
    "ANTIGRAVITY_HOME",
}


def parse_environment_file(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    if not path.exists():
        return values
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, encoded = line.split("=", 1)
        key = key.strip()
        if key not in SUPPORTED_KEYS:
            continue
        lexer = shlex.shlex(encoded, posix=True)
        lexer.whitespace_split = True
        lexer.commenters = "#"
        tokens = list(lexer)
        if len(tokens) != 1 or not tokens[0]:
            raise ValueError(f"{path}:{number}: {key} must contain one non-empty path")
        values[key] = tokens[0]
    return values


def normalize_session_root(value: str, source: str) -> Path:
    if any(ord(character) < 32 for character in value):
        raise ValueError(f"{source} contains a control character")
    path = Path(os.path.normpath(value))
    if not path.is_absolute():
        raise ValueError(f"{source} must be an absolute path")
    if path in FORBIDDEN_ROOTS:
        raise ValueError(f"{source} is too broad for ReadWritePaths: {path}")
    if source == "DSH_HOME" and path in DEFAULT_DSH_HOMES:
        return path
    if path in DEFAULT_ROOTS:
        return path
    current = Path("/")
    for part in path.parts[1:]:
        current /= part
        if current.is_symlink():
            raise ValueError(f"{source} traverses a symlink: {current}")
    return path


def configured_roots(values: dict[str, str], config: dict | None = None) -> dict[str, Path]:
    roots = {
        key: normalize_session_root(value, key)
        for key, value in values.items()
        if key in SUPPORTED_KEYS and not (key == "CODEX_HOME" and value == "auto")
    }
    configured = (config or {}).get("codex", {}).get("home", "")
    if configured not in ("", "auto"):
        roots["configured_codex"] = normalize_session_root(configured, "codex.home")
    return roots


def custom_session_roots(values: dict[str, str], config: dict | None = None) -> list[Path]:
    roots = set()
    for key, root in configured_roots(values, config).items():
        if key == "DSH_HOME":
            roots.update(normalize_session_root(str(root / name), key) for name in ("sessions", "storages"))
        else:
            roots.add(root)
    return sorted(roots - DEFAULT_ROOTS, key=str)


def systemd_quote(value: str) -> str:
    return '"' + value.replace("%", "%%").replace("\\", "\\\\").replace('"', '\\"') + '"'


def render(values: dict[str, str], config: dict | None = None) -> str:
    roots = custom_session_roots(values, config)
    if not roots:
        return ""
    lines = ["[Service]"]
    lines.extend(f"ReadWritePaths={systemd_quote('-' + str(root))}" for root in roots)
    for key, root in configured_roots(values, config).items():
        if root not in DEFAULT_ROOTS:
            for name in PROTECTED_FILES.get(key, ()):
                lines.append(f"ReadOnlyPaths={systemd_quote('-' + str(root / name))}")
    return "\n".join(lines) + "\n"


def main() -> int:
    if len(sys.argv) != 4:
        print("usage: provider-session-write-paths.py ENV_FILE CONFIG_FILE OUTPUT", file=sys.stderr)
        return 2
    env_path = Path(sys.argv[1])
    config_path = Path(sys.argv[2])
    output_path = Path(sys.argv[3])
    try:
        config = tomllib.loads(config_path.read_text(encoding="utf-8")) if config_path.exists() else {}
        output = render(parse_environment_file(env_path), config)
    except (OSError, ValueError) as error:
        print(f"provider session write path error: {error}", file=sys.stderr)
        return 1
    output_path.write_text(output, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
