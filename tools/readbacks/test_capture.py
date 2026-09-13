import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import ci


class CaptureFailures(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.checkout = self.root / "checkout"
        self.harness = self.root / "harness"
        self.checkout.mkdir()
        self.harness.mkdir()
        (self.checkout / "fast3d/goldens").mkdir(parents=True)
        (self.checkout / "fast3d/goldens/quad.bin").write_bytes(bytes([1, 2, 3, 4]))
        (self.checkout / "Cargo.lock").write_text("frozen test lock")
        (self.checkout / "fast3d/Cargo.toml").write_text("[dev-dependencies]\n")
        (self.checkout / "test-harness").write_text("test input")
        (self.harness / "harness-files.json").write_text('["test-harness"]')
        (self.harness / "goldens.json").write_text(json.dumps({
            "fast3d/goldens/quad.bin": "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"}))
        self.output = self.root / "output"

    def external_metadata(self, command, cwd, text, encoding):
        if command == ["git", "rev-parse", "HEAD"]:
            return "a" * 40
        if command == ["rustc", "-Vv"]:
            return "rustc test\nhost: aarch64-apple-darwin"
        if command[:2] == ["cargo", "tree"]:
            return "fast3d v1.0.0"
        if command[:2] == ["cargo", "metadata"]:
            return json.dumps({"resolve": {"nodes": [{"id": "fast3d", "features": [], "deps": []}]}})
        raise AssertionError(command)

    def capture(self, runner):
        with patch.object(ci, "HERE", self.harness), \
                patch.object(ci.subprocess, "check_output", self.external_metadata), \
                patch.object(ci.subprocess, "run", runner), \
                patch.dict(os.environ, {"GITHUB_RUN_ID": "1", "GITHUB_RUN_ATTEMPT": "1"}):
            ci.capture(self.checkout, self.output, "a" * 40, self.checkout)

    def test_test_failure_keeps_logs_and_manifest_for_all_configurations(self):
        def runner(command, **kwargs):
            default = "--all-features" not in command and "--features" not in command
            return subprocess.CompletedProcess(command, 101 if default else 0, stdout="build failed\n" if default else "")
        self.capture(runner)
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(set(manifest["configurations"]), {"default", "debug-ui", "all-features"})
        self.assertEqual(manifest["configurations"]["default"]["exit_code"], 101)
        self.assertEqual(manifest["configurations"]["all-features"]["exit_code"], 0)
        self.assertEqual((self.output / "default/build.log").read_text(), "build failed\n")
        self.assertTrue((self.output / "debug-ui/test.log").is_file())

    def test_failed_test_names_and_targets_are_retained(self):
        def runner(command, **kwargs):
            if "--no-run" in command:
                return subprocess.CompletedProcess(command, 0, stdout="")
            self.assertIn("--no-fail-fast", command)
            kwargs["stdout"].write("test tmem_layouts ... FAILED\n"
                                   "error: test failed, to rerun pass `-p fast3d --test tmem_fixture_replay`\n")
            return subprocess.CompletedProcess(command, 101)
        self.capture(runner)
        manifest = json.loads((self.output / "manifest.json").read_text())
        for result in manifest["configurations"].values():
            self.assertEqual(result["failed_tests"], ["tmem_layouts"])
            self.assertEqual(result["failed_targets"], ["`-p fast3d --test tmem_fixture_replay`"])
        with self.assertRaisesRegex(ValueError, "failed configuration: default.*tmem_layouts.*test.log"):
            ci.load_capture(self.output, ci.INVENTORY, "a" * 40, "1")

    def test_subprocess_failure_retains_source_provenance(self):
        def runner(command, **kwargs):
            raise OSError("runner unavailable")
        with self.assertRaisesRegex(OSError, "runner unavailable"):
            self.capture(runner)
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(manifest["source_sha"], "a" * 40)
        self.assertIsNone(manifest["configurations"]["default"]["exit_code"])

    def test_dependency_artifacts_are_authenticated(self):
        def runner(command, **kwargs):
            return subprocess.CompletedProcess(command, 0, stdout="")
        self.capture(runner)
        for name, replacement, error in [
                ("dependencies.txt", "changed tree", "full dependency graph hash mismatch"),
                ("test-dependencies.json", "[]", "test dependency graph hash mismatch")]:
            with self.subTest(name=name):
                path = self.output / name
                original = path.read_text()
                path.write_text(replacement)
                with self.assertRaisesRegex(ValueError, error):
                    ci.load_capture(self.output, ci.INVENTORY, "a" * 40, "1")
                path.write_text(original)

    def test_existing_evidence_is_unchanged_by_a_rejected_capture(self):
        self.output.mkdir()
        evidence = self.output / "failure.json"
        evidence.write_bytes(b"original evidence")
        arguments = ["ci.py", "--checkout", str(self.checkout), "--output", str(self.output),
                     "--base-sha", "a" * 40, "--artifact-name", "readbacks-windows-latest"]
        with patch.object(sys, "argv", arguments):
            self.assertEqual(ci.main(), 1)
        self.assertEqual(evidence.read_bytes(), b"original evidence")
