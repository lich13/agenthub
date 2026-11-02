#!/usr/bin/env python3
"""Synthetic regression cases; no private deployment values are stored here."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SCRIPT = Path(__file__).with_name("privacy-check.py").resolve()
spec = importlib.util.spec_from_file_location("privacy", SCRIPT)
privacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(privacy)


class PrivacyChecks(unittest.TestCase):
    def test_reserved_examples_and_public_repository_are_allowed(self):
        self.assertEqual(privacy.problems(b"https://panel.example.com/agenthub/ 192.0.2.10 /Users/example/project author@users.noreply.github.com https://github.com/lich13/agenthub", []), [])

    def test_unknown_endpoint_private_home_and_secret_are_rejected(self):
        data = b"https://" + b"personal" + b".invalid-host/ /Users/" + b"synthetic-person/project ghp_" + b"a" * 36
        rules = privacy.problems(data, [])
        self.assertIn("unreviewed_endpoint", rules)
        self.assertIn("private_home", rules)
        self.assertIn("credential", rules)

    def test_native_payload_checks_private_values_without_string_table_false_positives(self):
        data = b"\x7fELF\01.4.0.1\0http://concatenatedliteral\0fixture-private-value\0/Users/" + b"synthetic-person/source"
        self.assertEqual(privacy.problems(data, ["fixture-private-value"]), ["private_home", "private_value"])

    def test_native_payload_rejects_public_ipv4_while_allowing_loopback(self):
        loopback_elf = b"\x7fELF\0" + b"127.0.0.1:8080\0"
        self.assertEqual(privacy.problems(loopback_elf, []), [])
        public_ip_elf = b"\x7fELF\0" + b".".join([b"7", b"0", b"0", b"1"]) + b":80\0"
        self.assertEqual(privacy.problems(public_ip_elf, []), ["public_ip"])

    def test_generated_frontend_skips_lexical_url_false_positives(self):
        bundle = b'"ht' + b'tps://www.css";const source="https://service.example.invalid";'
        self.assertIn("unreviewed_endpoint", privacy.problems(bundle, []))
        self.assertEqual(privacy.problems(bundle, [], scan_endpoint_literals=False), [])


class PrivacyCommandChecks(unittest.TestCase):
    """Run the real CLI against isolated repositories and synthetic payloads."""

    def setUp(self):
        sandbox = tempfile.TemporaryDirectory(prefix="agenthub-privacy-test-")
        self.addCleanup(sandbox.cleanup)
        self.base = Path(sandbox.name).resolve()
        self.root = self.base / "repository"
        self.root.mkdir()
        self.env = {
            key: value for key, value in os.environ.items()
            if not key.startswith("GIT_")
        }
        self.env.update({
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_AUTHOR_NAME": "Privacy Fixture",
            "GIT_AUTHOR_EMAIL": "fixture@users.noreply.github.com",
            "GIT_COMMITTER_NAME": "Privacy Fixture",
            "GIT_COMMITTER_EMAIL": "fixture@users.noreply.github.com",
            "GIT_AUTHOR_DATE": "2000-01-01T00:00:00+00:00",
            "GIT_COMMITTER_DATE": "2000-01-01T00:00:00+00:00",
            "PYTHONDONTWRITEBYTECODE": "1",
            "PYTHONIOENCODING": "utf-8",
            "LC_ALL": "C",
        })
        self.git("init", "--quiet", "--initial-branch=main")

    def git(self, *args, data=None, env=None):
        result = subprocess.run(
            ["git", "-c", "core.hooksPath=" + os.devnull,
             "-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false",
             "-c", "core.autocrlf=false", "-C", str(self.root), *args],
            input=data, capture_output=True, env={**self.env, **(env or {})},
            timeout=15,
        )
        self.assertEqual(result.returncode, 0, "synthetic Git fixture setup failed")
        return result.stdout.decode().strip()

    def write(self, path, contents):
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(contents, bytes):
            path.write_bytes(contents)
        else:
            path.write_text(contents, encoding="utf-8")
        return path

    def private_values(self, *values):
        return self.write(self.base / "private-values.json", json.dumps(values))

    def run_check(self, *args, root=None, wrapper=None):
        command = [sys.executable]
        if wrapper is not None:
            command += ["-c", wrapper]
        command += [str(SCRIPT), "--root", str(root or self.root),
                    *map(str, args)]
        return subprocess.run(
            command, capture_output=True, text=True, env=self.env,
            cwd=self.base, timeout=15,
        )

    def assert_redacted(self, result, *values):
        output = result.stdout + result.stderr
        for value in (str(self.base), str(SCRIPT), *values):
            # Do not expose captured output or a fixture value on assertion failure.
            self.assertFalse(value in output, "scanner output exposed a protected value")

    def report(self, result, expected_exit, *values):
        self.assert_redacted(result, *values)
        self.assertEqual(result.returncode, expected_exit, "unexpected scanner exit code")
        self.assertFalse(result.stderr, "normal scanning wrote a diagnostic to stderr")
        try:
            report = json.loads(result.stdout)
        except json.JSONDecodeError:
            self.fail("scanner did not emit a JSON report")
        self.assertEqual(set(report), {"checked", "failures"})
        self.assertIsInstance(report["checked"], int)
        self.assertIsInstance(report["failures"], list)
        for failure in report["failures"]:
            self.assertEqual(set(failure), {"source", "rule"})
        return report

    def assert_safe_error(self, result, *values):
        self.assertNotEqual(result.returncode, 0, "scanner accepted invalid input")
        self.assert_redacted(result, *values)
        output = result.stdout + result.stderr
        self.assertTrue(output.strip(), "scanner rejected input without a diagnostic")
        self.assertFalse("Traceback" in output, "scanner exposed an exception traceback")
        self.assertFalse("fatal:" in output, "scanner forwarded a raw Git diagnostic")

    def test_files_include_tracked_and_untracked_but_exclude_ignored(self):
        marker = "fixture-file-content-marker"
        self.write(self.root / ".gitignore", "ignored.txt\n")
        self.write(self.root / "tracked.txt", marker)
        self.git("add", ".gitignore", "tracked.txt")
        self.write(self.root / "nested" / "untracked.txt", marker)
        self.write(self.root / "ignored.txt", marker)

        report = self.report(
            self.run_check("--private-values", self.private_values(marker)), 1, marker,
        )
        self.assertEqual(report["checked"], 3)
        self.assertEqual(report["failures"], [
            {"source": "nested/untracked.txt", "rule": "private_value"},
            {"source": "tracked.txt", "rule": "private_value"},
        ])

    def test_deleted_files_and_symlinks_do_not_read_external_contents(self):
        marker = "fixture-external-content-marker"
        target = self.write(self.base / "external.txt", marker)
        self.write(self.root / "deleted.txt", marker)
        (self.root / "tracked-link.txt").symlink_to(target)
        self.git("add", "deleted.txt", "tracked-link.txt")
        (self.root / "deleted.txt").unlink()
        (self.root / "untracked-link.txt").symlink_to(target)
        (self.root / "missing-link.txt").symlink_to(self.base / "missing.txt")

        report = self.report(
            self.run_check("--private-values", self.private_values(marker)), 0, marker,
        )
        self.assertEqual(report, {"checked": 0, "failures": []})

    def test_file_rule_output_reports_location_and_categories_without_values(self):
        endpoint = "https://" + "fixture" + ".invalid-host/"
        private_home = "/Users/" + "synthetic-person/project"
        secret = "ghp_" + "a" * 36
        marker = "fixture-explicit-private-marker"
        self.write(self.root / "content.txt", "\n".join((endpoint, private_home, secret, marker)))

        report = self.report(
            self.run_check("--private-values", self.private_values(marker)), 1,
            endpoint, private_home, secret, marker,
        )
        self.assertEqual(report["failures"], [
            {"source": "content.txt", "rule": rule}
            for rule in ("credential", "private_home", "private_value", "unreviewed_endpoint")
        ])

    def test_git_objects_scan_history_other_refs_and_annotated_tags(self):
        old_marker = "fixture-historical-blob-marker"
        branch_marker = "fixture-other-branch-marker"
        tag_marker = "fixture-annotated-tag-marker"
        unreachable_marker = "fixture-unreachable-blob-marker"
        self.write(self.root / "history.txt", old_marker)
        self.git("add", "history.txt")
        self.git("commit", "--quiet", "-m", "Initial synthetic content")
        old_blob = self.git("rev-parse", "HEAD:history.txt")
        self.write(self.root / "history.txt", "Current clean fixture\n")
        self.git("commit", "--quiet", "-am", "Replace synthetic content")
        self.git("checkout", "--quiet", "-b", "fixture-side")
        self.write(self.root / "branch.txt", branch_marker)
        self.git("add", "branch.txt")
        self.git("commit", "--quiet", "-m", "Other synthetic branch")
        branch_blob = self.git("rev-parse", "HEAD:branch.txt")
        self.git("checkout", "--quiet", "main")
        self.git("tag", "--annotate", "fixture-tag", "--message", tag_marker)
        tag_object = self.git("rev-parse", "refs/tags/fixture-tag")
        unreachable = self.git("hash-object", "-w", "--stdin", data=unreachable_marker.encode())
        values = (old_marker, branch_marker, tag_marker, unreachable_marker)

        report = self.report(
            self.run_check("--git-objects", "--private-values", self.private_values(*values)),
            1, *values,
        )
        self.assertCountEqual(report["failures"], [
            {"source": "git:" + oid, "rule": "private_value"}
            for oid in (old_blob, branch_blob, tag_object)
        ])
        self.assertFalse(any(item["source"] == "git:" + unreachable for item in report["failures"]))

    def test_git_objects_reject_commit_and_tag_identities_without_echoing_them(self):
        email = "fixture@example.invalid"
        self.git("commit", "--quiet", "--allow-empty", "-m", "Synthetic identity", env={
            "GIT_AUTHOR_EMAIL": email, "GIT_COMMITTER_EMAIL": email,
        })
        commit = self.git("rev-parse", "HEAD")
        self.git("tag", "--annotate", "fixture-identity", "--message", "Synthetic tag", env={
            "GIT_COMMITTER_EMAIL": email,
        })
        tag = self.git("rev-parse", "refs/tags/fixture-identity")

        report = self.report(self.run_check("--git-objects"), 1, email)
        self.assertCountEqual(report["failures"], [
            {"source": "git:" + commit, "rule": "commit_identity"},
            {"source": "git:" + commit, "rule": "commit_identity"},
            {"source": "git:" + tag, "rule": "commit_identity"},
        ])

    def test_git_objects_accept_clean_noreply_history(self):
        self.write(self.root / "content.txt", "https://service.example.invalid/\n")
        self.git("add", "content.txt")
        self.git("commit", "--quiet", "-m", "Clean synthetic history")
        self.git("tag", "--annotate", "fixture-clean", "--message", "Clean synthetic tag")

        report = self.report(self.run_check("--git-objects"), 0)
        self.assertEqual(report, {"checked": 4, "failures": []})

    def test_payloads_scan_nested_files_and_multiple_roots_without_following_symlinks(self):
        marker = "fixture-payload-marker"
        payload = self.base / "payload"
        other_payload = self.base / "other-payload"
        self.write(payload / "nested" / "content.txt", marker)
        self.write(other_payload / "native.bin", b"\x7fELF\0" + marker.encode())
        external = self.write(self.base / "external.txt", marker)
        (payload / "external-link.txt").symlink_to(external)
        (payload / "missing-link.txt").symlink_to(self.base / "missing.txt")
        (payload / "directory-link").symlink_to(other_payload, target_is_directory=True)

        report = self.report(self.run_check(
            "--payload", payload, "--payload", other_payload,
            "--private-values", self.private_values(marker),
        ), 1, marker)
        self.assertEqual(report["checked"], 2)
        self.assertCountEqual(report["failures"], [
            {"source": "payload:nested/content.txt", "rule": "private_value"},
            {"source": "payload:native.bin", "rule": "private_value"},
        ])

    def test_generated_payloads_relax_only_endpoint_literal_checks(self):
        endpoint = "https://" + "fixture" + ".invalid-host/"
        secret = "ghp_" + "b" * 36
        contents = endpoint + "\n" + secret
        payload = self.base / "payload"
        for filename in ("bundle.js", "styles.CSS", "content.txt"):
            self.write(payload / filename, contents)
        self.write(self.root / "source.js", endpoint)

        report = self.report(self.run_check("--payload", payload), 1, endpoint, secret)
        self.assertCountEqual(report["failures"], [
            {"source": "source.js", "rule": "unreviewed_endpoint"},
            {"source": "payload:bundle.js", "rule": "credential"},
            {"source": "payload:styles.CSS", "rule": "credential"},
            {"source": "payload:content.txt", "rule": "credential"},
            {"source": "payload:content.txt", "rule": "unreviewed_endpoint"},
        ])

    def test_empty_private_values_and_existing_empty_payload_are_allowed(self):
        payload = self.base / "payload"
        payload.mkdir()
        self.write(self.root / "content.txt", "Clean synthetic content\n")
        report = self.report(self.run_check(
            "--payload", payload, "--private-values", self.private_values(""),
        ), 0)
        self.assertEqual(report, {"checked": 1, "failures": []})

    def test_private_values_must_remain_outside_the_repository(self):
        marker = "fixture-private-list-marker"
        values = self.write(self.root / "values.json", json.dumps([marker]))
        alias = self.base / "values-link.json"
        alias.symlink_to(values)
        for path in (values, alias):
            with self.subTest(location=path.name):
                self.assert_safe_error(self.run_check("--private-values", path), marker)

    def test_private_values_require_a_json_string_list(self):
        marker = "fixture-invalid-schema-marker"
        for name, value in (("object", {"value": marker}), ("string", marker),
                            ("mixed_list", [marker, 3]), ("null", None)):
            with self.subTest(schema=name):
                path = self.write(self.base / "values.json", json.dumps(value))
                self.assert_safe_error(self.run_check("--private-values", path), marker)

    def test_private_value_read_and_decode_errors_are_sanitized(self):
        marker = "fixture-input-error-marker"
        malformed = self.write(self.base / "malformed.json", marker)
        invalid_encoding = self.write(self.base / "invalid-encoding.json", b"\xff" + marker.encode())
        for name, path in (("missing", self.base / (marker + ".json")),
                           ("malformed", malformed), ("encoding", invalid_encoding)):
            with self.subTest(error=name):
                self.assert_safe_error(self.run_check("--private-values", path), marker)

    def test_missing_or_non_directory_payloads_are_rejected_safely(self):
        marker = "fixture-payload-error-marker"
        regular_file = self.write(self.base / "payload.txt", marker)
        for name, path in (("missing", self.base / marker), ("file", regular_file)):
            with self.subTest(kind=name):
                self.assert_safe_error(self.run_check("--payload", path), marker)

    def test_invalid_repository_errors_are_sanitized(self):
        marker = "fixture-repository-error-marker"
        non_repository = self.base / "not-a-repository"
        non_repository.mkdir()
        for name, path in (("missing", self.base / marker), ("not_git", non_repository)):
            with self.subTest(kind=name):
                self.assert_safe_error(self.run_check(root=path), marker)

    def test_file_read_errors_are_sanitized(self):
        marker = "fixture-file-read-error-marker"
        self.write(self.root / "content.txt", "Clean synthetic content\n")
        wrapper = "\n".join((
            "import runpy, sys",
            "from unittest import mock",
            "script = sys.argv.pop(1)",
            "sys.argv[0] = script",
            "with mock.patch('pathlib.Path.read_bytes', side_effect=OSError(" + repr(marker) + ")):",
            "    runpy.run_path(script, run_name='__main__')",
        ))
        self.assert_safe_error(self.run_check(wrapper=wrapper), marker)


if __name__ == "__main__":
    unittest.main()
