#!/usr/bin/env python3
"""Exercise provider mount rules, optionally inside a real systemd namespace."""
import argparse
import importlib.util
import json
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
parser = argparse.ArgumentParser()
parser.add_argument("--module", type=Path, default=Path(__file__).resolve().parents[1] / "deploy/agenthub-webd/provider-session-write-paths.py")
parser.add_argument("--systemd", action="store_true")
args = parser.parse_args()
spec = importlib.util.spec_from_file_location("provider_paths", args.module)
provider_paths = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provider_paths)


class ProviderWritePathsTests(unittest.TestCase):
    def test_default_roots_are_in_the_base_unit(self):
        self.assertEqual(provider_paths.render({}), "")
        self.assertEqual(provider_paths.render({"GROK_HOME": "/root/.grok", "CLAUDE_CONFIG_DIR": "/root/.claude"}), "")
        self.assertEqual(provider_paths.render({"CODEX_HOME": "auto"}, {"codex": {"home": "auto"}}), "")

    def test_dsh_only_grants_sessions_and_storages_including_missing_directories(self):
        self.assertEqual(provider_paths.render({"DSH_HOME": "/root/.dsh"}), "")
        output = provider_paths.render({"DSH_HOME": "/srv/missing-fixture-dsh"})
        self.assertEqual(output, '[Service]\nReadWritePaths="-/srv/missing-fixture-dsh/sessions"\nReadWritePaths="-/srv/missing-fixture-dsh/storages"\n')
        unit = (args.module.parent / "systemd.service").read_text()
        self.assertIn("-/root/.dsh/sessions -/root/.dsh/storages", unit)
        self.assertNotIn("-/root/.dsh ", unit)
        for path in provider_paths.DEFAULT_ROOTS:
            if "/.dsh/" in str(path):
                self.assertIn("-" + str(path), unit)

    def test_custom_roots_cover_indexes_and_protect_settings(self):
        values = {"GROK_HOME": "/srv/fixture-grok", "CLAUDE_CONFIG_DIR": "/srv/fixture-claude", "CODEX_HOME": "/srv/fixture-codex"}
        output = provider_paths.render(values, {"codex": {"home": "/srv/configured-codex"}})
        for value in [*values.values(), "/srv/configured-codex"]:
            self.assertIn(f'ReadWritePaths="-{value}"\n', output)
        for key, names in provider_paths.PROTECTED_FILES.items():
            for name in names:
                self.assertIn(f'ReadOnlyPaths="-{values[key]}/{name}"\n', output)
        self.assertNotIn("/sessions\"", output)
        self.assertNotIn("/projects\"", output)

    def test_config_changes_regenerate_the_dropin_and_missing_roots_are_optional(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            env = root / "env"
            config = root / "config.toml"
            output = root / "paths.conf"
            env.write_text('CLAUDE_CONFIG_DIR="/srv/missing fixture"\nUNRELATED_KEY=ignored\n')
            config.write_text('[codex]\nhome="/srv/old-codex"\n')
            command = [sys.executable, str(args.module), str(env), str(config), str(output)]
            subprocess.run(command, check=True, capture_output=True)
            self.assertIn('ReadWritePaths="-/srv/missing fixture"', output.read_text())
            config.write_text('[codex]\nhome="/srv/new-codex"\n')
            subprocess.run(command, check=True, capture_output=True)
            self.assertNotIn("old-codex", output.read_text())
            self.assertIn("new-codex", output.read_text())

    def test_root_validation_and_systemd_literal_escaping(self):
        for root in ["/", "/root", "/srv/../home", "relative", "auto", "", "/srv/fixture\nroot"]:
            with self.assertRaises(ValueError):
                provider_paths.render({"GROK_HOME": root})
        self.assertIn("fixture%%name", provider_paths.render({"GROK_HOME": "/srv/fixture%name"}))
        with tempfile.TemporaryDirectory() as temporary:
            link = Path(temporary) / "link"
            link.symlink_to(Path(temporary) / "missing")
            with self.assertRaises(ValueError):
                provider_paths.render({"CLAUDE_CONFIG_DIR": str(link)})


def systemd_fixture():
    # Every write belongs to this temporary fixture, never a native provider.
    with tempfile.TemporaryDirectory(prefix="agenthub-provider-test-", dir=Path.home()) as temporary:
        root = Path(temporary)
        values = {"GROK_HOME": str(root / "grok"), "CLAUDE_CONFIG_DIR": str(root / "claude"), "CODEX_HOME": str(root / "codex")}
        for key, value in values.items():
            directory = Path(value)
            directory.mkdir()
            (directory / "sessions").mkdir()
            (directory / "projects").mkdir()
            (directory / "active_sessions.json").write_text('[{"id":"selected"},{"id":"retained"}]\n')
            (directory / "history.jsonl").write_text('{"id":"selected"}\n{"id":"retained"}\n')
            for name in provider_paths.PROTECTED_FILES.get(key, ()):
                (directory / name).write_text("protected fixture\n")
        worker = root / "worker.py"
        worker.write_text('''import errno, json, os, sys
from pathlib import Path
roots=json.loads(sys.argv[1]); protected=json.loads(sys.argv[2]); legacy=sys.argv[3]=='legacy'
for key, value in roots.items():
    root=Path(value)
    index=root/'active_sessions.json'
    temporary=root/'.agenthub-index-fixture'
    try:
        temporary.write_text(json.dumps([row for row in json.loads(index.read_text()) if row['id']!='selected']))
        os.replace(temporary,index)
    except OSError as error:
        assert legacy and key!='CODEX_HOME' and error.errno==errno.EROFS, (key,error)
        continue
    assert not legacy or key=='CODEX_HOME'
    assert json.loads(index.read_text())==[{'id':'retained'}]
    history=root/'history.jsonl'
    temporary.write_text(''.join(line for line in history.read_text().splitlines(keepends=True) if json.loads(line)['id']!='selected'))
    os.replace(temporary,history)
    assert history.read_text()=='{"id":"retained"}\\n'
    session=root/'sessions'/'selected'; session.mkdir(); (session/'fixture').write_text('fixture')
    (session/'fixture').unlink(); session.rmdir()
    for name in protected.get(key,[]):
        path=root/name
        try: path.write_text('unexpected')
        except OSError as error: assert error.errno==errno.EROFS
        else: raise AssertionError('protected file became writable')
        temporary.write_text('unexpected')
        try: os.replace(temporary,path)
        except OSError as error: assert error.errno in (errno.EROFS,errno.EBUSY)
        else: raise AssertionError('protected file was replaced')
        temporary.unlink()
print('namespace '+sys.argv[3]+': ok')
''')
        for legacy in (True, False):
            writes = [value + ("/sessions" if legacy and key != "CODEX_HOME" else "") for key, value in values.items()]
            properties = ["ProtectSystem=full", "ProtectHome=read-only", "NoNewPrivileges=true", "PrivateTmp=true",
                          "ReadWritePaths=" + " ".join([*writes, "-" + str(root / "absent-provider")])]
            if not legacy:
                generated = {"ReadWritePaths": [], "ReadOnlyPaths": []}
                for line in provider_paths.render(values).splitlines()[1:]:
                    key, value = line.split("=", 1)
                    generated[key].extend(shlex.split(value))
                properties = properties[:-1] + [key + "=" + " ".join(paths) for key, paths in generated.items()]
                properties[4] += " -" + str(root / "absent-provider")
            command = ["systemd-run", "--quiet", "--wait", "--pipe", "--collect", *["--property=" + value for value in properties], sys.executable, str(worker), json.dumps(values), json.dumps(provider_paths.PROTECTED_FILES), "legacy" if legacy else "fixed"]
            completed = subprocess.run(command, capture_output=True, text=True)
            if completed.returncode:
                raise RuntimeError(completed.stderr + completed.stdout)
            print(completed.stdout.strip())


if __name__ == "__main__":
    if args.systemd:
        systemd_fixture()
    else:
        unittest.main(argv=[sys.argv[0]])
