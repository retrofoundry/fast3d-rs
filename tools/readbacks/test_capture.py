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
        (self.checkout / "ultragfx/goldens").mkdir(parents=True)
        (self.checkout / "ultragfx/goldens/quad.bin").write_bytes(bytes([1, 2, 3, 4]))
        (self.checkout / "Cargo.lock").write_text("frozen test lock")
        (self.checkout / "ultragfx/Cargo.toml").write_text("[dev-dependencies]\n")
        (self.checkout / "test-harness").write_text("test input")
        (self.harness / "harness-files.json").write_text('["test-harness"]')
        (self.harness / "goldens.json").write_text(json.dumps({
            "ultragfx/goldens/quad.bin": "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"}))
        self.output = self.root / "output"

    def external_metadata(self, command, cwd, text, encoding):
        if command == ["git", "rev-parse", "HEAD"]:
            return "a" * 40
        if command == ["rustc", "-Vv"]:
            return "rustc test\nhost: aarch64-apple-darwin"
        if command[:2] == ["cargo", "tree"]:
            return "ultragfx v1.0.0"
        if command[:2] == ["cargo", "metadata"]:
            return json.dumps({"resolve": {"nodes": [{"id": "ultragfx", "features": [], "deps": []}]}})
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
                                   "error: test failed, to rerun pass `-p ultragfx --test tmem_fixture_replay`\n")
            return subprocess.CompletedProcess(command, 101)
        self.capture(runner)
        manifest = json.loads((self.output / "manifest.json").read_text())
        for result in manifest["configurations"].values():
            self.assertEqual(result["failed_tests"], ["tmem_layouts"])
            self.assertEqual(result["failed_targets"], ["`-p ultragfx --test tmem_fixture_replay`"])
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

    def test_windows_capture_bounds_targets_and_keeps_failures_and_build_cleanup(self):
        tests = self.checkout / "ultragfx/tests"
        tests.mkdir()
        for target in ["tmem_fixture_replay", "texture_decode_gpu", "texture_residency", "texture_residency_pressure"]:
            (tests / (target + ".rs")).write_text("test target")
        targets = set()
        executions = []

        def runner(command, **kwargs):
            target = Path(kwargs["env"]["CARGO_TARGET_DIR"])
            self.assertTrue(target.is_dir())
            targets.add(target)
            (target / "build-product").write_text("temporary build output")
            if "--no-run" in command:
                self.assertNotIn("--", command)
                return subprocess.CompletedProcess(command, 0, stdout="")
            executions.append(command)
            if "--lib" in command:
                selected = [
                    "tests::goldens::", "tests::tmem_witnesses::",
                    "render::texture_decode::gpu_tests::",
                    "tests::framebuffer_coherence::gpu::guest_route_",
                    "tests::framebuffer_coherence::gpu::triangles::guest_route_gpu_triangle",
                    "tests::framebuffer_coherence::gpu::triangles::batches::guest_route_gpu_batch",
                    "tests::framebuffer_coherence::gpu::compatibility_literal_fill_matches_f4b3240",
                    "tests::framebuffer_coherence::gpu::compatibility_literal_fill_then_invalid_projection_matches_67345dc",
                    "hle::capture::tests::legacy_scales_are_ignored_and_v3_preserves_effective_scale",
                    "hle::capture::replay::state_tests::captures_record_the_effective_fixed_scale",
                    "hle::capture::replay::sequence_tests::replay_options_cover_the_prefix_and_selected_crop"]
                if "--features" not in command and "--all-features" not in command:
                    selected.extend(["tests::fog::", "tests::fog_mirror::"])
                self.assertEqual(command[command.index("--") + 1:], selected)
                return subprocess.CompletedProcess(command, 101)
            for name in ["tmem_fixture_replay", "texture_decode_gpu", "texture_residency", "texture_residency_pressure"]:
                self.assertIn(name, command)
            self.assertIn("--all-features", command)
            return subprocess.CompletedProcess(command, 0)

        with patch.object(ci.platform, "system", return_value="Windows"):
            self.capture(runner)
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(len(executions), 4)
        self.assertEqual(manifest["gpu_decode"]["profile"], "warp")
        for result in manifest["configurations"].values():
            self.assertEqual(result["exit_code"], 101)
        self.assertEqual(len(targets), 1)
        self.assertFalse(next(iter(targets)).exists())

    def test_windows_base_only_selects_integration_targets_in_its_source(self):
        commands = ci.test_commands(self.checkout, "all-features", "Windows")
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[1][-2:], ["--test", "tmem_fixture_replay"])
        for config in ci.COMMANDS:
            self.assertEqual(ci.test_commands(self.checkout, config, "Darwin"), [ci.COMMANDS[config]])

    def test_windows_selects_scaled_feedback_and_public_api_when_present(self):
        for name in ["ultragfx/src/render/upscaling_tests.rs", "ultragfx/src/render/framebuffers/copies_tests.rs", "ultragfx/tests/upscaling.rs"]:
            path = self.checkout / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        commands = ci.test_commands(self.checkout, "all-features", "Windows")
        self.assertIn("render::upscaling_tests::", commands[0])
        self.assertIn("render::framebuffers::copies::tests::", commands[0])
        self.assertIn("hle::capture::tests::legacy_scales_are_ignored_and_v3_preserves_effective_scale", commands[0])
        self.assertIn("hle::capture::replay::state_tests::captures_record_the_effective_fixed_scale", commands[0])
        self.assertIn("hle::capture::replay::sequence_tests::replay_options_cover_the_prefix_and_selected_crop", commands[0])
        self.assertIn("upscaling", commands[1])

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
