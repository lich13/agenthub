#!/usr/bin/env python3
"""Exercise signed/unsigned hooks and reject server assets in skip mode."""
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("build_config", Path(__file__).with_name("tauri-build-config.py"))
build_config = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_config)


class BuildConfigTests(unittest.TestCase):
    def test_macos_signing_identity_override_is_scoped(self):
        source = Path(__file__).resolve().parents[1] / "src-tauri" / "tauri.conf.json"
        original = json.loads(source.read_text())
        original_updater = original["plugins"]["updater"]
        self.assertEqual(original["bundle"]["macOS"]["signingIdentity"], "-")

        with tempfile.TemporaryDirectory(prefix="agenthub-build-config-") as directory:
            output = Path(directory) / "output.json"
            webui = source.parent.parent / "webui"
            fixture_identity = "fixture-apple-signing-identity"

            for signed in (True, False):
                with self.subTest(signed=signed), patch.dict(os.environ, {"MACOS_CODESIGN_IDENTITY": fixture_identity}):
                    build_config.prepare(source, output, webui, signed, False)
                    result = json.loads(output.read_text())
                    self.assertEqual(result["bundle"]["macOS"]["signingIdentity"], fixture_identity)
                    self.assertEqual(result["bundle"]["createUpdaterArtifacts"], signed)
                    if signed:
                        self.assertEqual(result["plugins"]["updater"], original_updater)
                    else:
                        self.assertNotIn("updater", result["plugins"])
                    self.assertNotIn(fixture_identity, json.dumps(result["plugins"]))
                    self.assertEqual(json.loads(source.read_text()), original)

            with patch.dict(os.environ, {"MACOS_CODESIGN_IDENTITY": ""}):
                build_config.prepare(source, output, webui, True, False)
            result = json.loads(output.read_text())
            self.assertEqual(result["bundle"]["macOS"]["signingIdentity"], "-")
            self.assertEqual(result["plugins"]["updater"], original_updater)
            self.assertEqual(result["bundle"]["createUpdaterArtifacts"], True)
            self.assertEqual(json.loads(source.read_text()), original)

    def test_hook_and_reuse_contract(self):
        with tempfile.TemporaryDirectory(prefix="agenthub-build-config-") as directory:
            root = Path(directory).resolve()
            source, output = root / "source.json", root / "output.json"
            original = {"build": {"beforeBuildCommand": "pnpm build:tauri", "frontendDist": "../wrong-server/dist"}, "bundle": {"createUpdaterArtifacts": True}, "plugins": {"updater": {"pubkey": "fixture"}}}
            source.write_text(json.dumps(original))
            assets = root / "dist" / "assets"
            assets.mkdir(parents=True)
            (assets / "main.js").write_text("export default 1")
            index = root / "dist" / "index.html"
            index.write_text('<script type="module" src="./assets/main.js"></script>')
            for signed in (True, False):
                for skip in (True, False):
                    with self.subTest(signed=signed, skip=skip):
                        build_config.prepare(source, output, root, signed, skip)
                        result = json.loads(output.read_text())
                        self.assertEqual(result["build"]["beforeBuildCommand"], "" if skip else {"script": "pnpm build:tauri", "cwd": str(root)})
                        self.assertEqual(
                            (source.parent / result["build"]["frontendDist"]).resolve(),
                            assets.parent,
                        )
                        self.assertFalse(Path(result["build"]["frontendDist"]).is_absolute())
                        self.assertEqual("updater" in result["plugins"], signed)
                        self.assertEqual(result["bundle"]["createUpdaterArtifacts"], signed)
                        self.assertEqual(json.loads(source.read_text()), original)
            for asset in ("/agenthub/assets/main.js", "/assets/main.js", "./assets/missing.js", "./../source.json", "https://example.com/main.js"):
                with self.subTest(asset=asset):
                    index.write_text(f'<script src="{asset}"></script>')
                    with self.assertRaises(ValueError):
                        build_config.prepare(source, output, root, True, True)
            index.write_text("<html></html>")
            with self.assertRaises(ValueError):
                build_config.prepare(source, output, root, True, True)
            index.unlink()
            with self.assertRaises(FileNotFoundError):
                build_config.prepare(source, output, root, True, True)


if __name__ == "__main__":
    unittest.main()
