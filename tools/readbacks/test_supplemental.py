import json
from pathlib import Path
import shutil
import unittest

import ci
import readbacks
from test_readbacks import BASE, HEAD, CaptureCase


class SupplementalReadbacks(CaptureCase):
    def setUp(self):
        super().setUp()
        self.gpu_inventory = {"version": 1, "rows": [{"id": "tile", "width": 1, "height": 1}]}
        self.checkout = self.root / "checkout"
        source = self.checkout / ci.GPU_DECODE_TEST
        source.parent.mkdir(parents=True)
        source.write_text("authored compute readback test")
        shutil.copyfile(source, self.head / "gpu-decode-source.rs")
        manifest = self.manifest(self.head)
        manifest["gpu_decode"] = ci.gpu_decode_requirement(self.checkout, system="Windows")
        for config in self.inventory["configurations"]:
            directory = self.head / config / "gpu-decode"
            directory.mkdir(parents=True)
            (directory / "tile.bin").write_bytes(bytes([17, 33, 65, 255]))
            (directory / "tile.expected.bin").write_bytes(bytes([17, 33, 65, 255]))
            (directory / "tile.input.bin").write_bytes(b"encoded bank and recipe")
            metadata = {"id": "tile", "width": 1, "height": 1, "channels": "RGBA",
                        "stage": "decode", "role": "gpu-compute", "blend_path": "none",
                        "test_id": "gpu_decode_readback", "adapter": manifest["rows"][0]["adapter"]}
            (directory / "tile.json").write_text(json.dumps(metadata))
        ci.save_gpu_decode_manifest(self.head, manifest, self.gpu_inventory)
        self.write(self.head / "manifest.json", manifest)

    @staticmethod
    def write(path, value):
        path.write_text(json.dumps(value))

    @staticmethod
    def manifest(root):
        return json.loads((root / "manifest.json").read_text())

    def mutate_gpu(self, change):
        path = self.head / "gpu-decode-manifest.json"
        manifest = json.loads(path.read_text())
        change(manifest)
        self.write(path, manifest)
        self.mutate(self.head, lambda m: m["gpu_decode"].update(manifest_sha256=readbacks.file_hash(path)))

    def load(self, **kwargs):
        return readbacks.load_capture(self.head, self.inventory, HEAD, "20",
                                      supplemental_inventory=self.gpu_inventory, **kwargs)

    def compare_gpu(self, **kwargs):
        return readbacks.compare(self.base, self.head, self.inventory, BASE, HEAD, "10", "20",
                                 supplemental_inventory=self.gpu_inventory, **kwargs)

    def test_complete_supplement_does_not_extend_frozen_parent_inventory(self):
        report = self.compare_gpu()
        self.assertTrue(report["passed"])
        self.assertEqual(len(report["rows"]), 3)
        self.assertEqual(report["gpu_decode"]["candidate"]["rows"], 3)
        self.assertEqual(report["gpu_decode"]["base"]["required_configurations"], [])

    def test_collector_authenticates_actual_input_expected_output_and_metadata(self):
        manifest = json.loads((self.head / "gpu-decode-manifest.json").read_text())
        self.assertEqual(manifest["inventory_sha256"], readbacks.json_hash(self.gpu_inventory))
        self.assertEqual(len(manifest["rows"]), 3)
        row = manifest["rows"][0]
        for file_field, hash_field in [("file", "sha256"), ("input_file", "input_sha256"),
                                       ("expected_file", "expected_sha256"), ("metadata_file", "metadata_sha256")]:
            self.assertEqual(readbacks.file_hash(self.head / row[file_field]), row[hash_field])

    def test_missing_row_cannot_pass_even_if_cargo_succeeded(self):
        self.mutate_gpu(lambda m: m["rows"].pop())
        with self.assertRaisesRegex(ValueError, "missing GPU decode rows"):
            self.load()
        report = self.compare_gpu(allow_incomplete=True)
        self.assertFalse(report["passed"])
        self.assertFalse(report["complete"])
        self.assertEqual(len(report["gpu_decode"]["candidate"]["missing_rows"]), 1)

    def test_all_features_alone_cannot_supply_three_configurations(self):
        self.mutate_gpu(lambda m: m.update(rows=[r for r in m["rows"] if r["configuration"] == "all-features"]))
        with self.assertRaisesRegex(ValueError, "missing GPU decode rows"):
            self.load()

    def test_failed_configuration_keeps_authenticated_partial_results(self):
        self.mutate(self.head, lambda m: m["configurations"]["default"].update(exit_code=101))
        self.mutate_gpu(lambda m: m.update(rows=[r for r in m["rows"] if r["configuration"] != "default"]))
        report = self.compare_gpu(allow_incomplete=True)
        self.assertFalse(report["passed"])
        self.assertEqual(report["gpu_decode"]["candidate"]["rows"], 2)
        self.assertIn("failed configuration", report["failures"]["candidate"][0])

    def test_missing_output_and_manifest_fail_authentication(self):
        for name in ["default/gpu-decode/tile.bin", "gpu-decode-manifest.json"]:
            with self.subTest(file=name):
                path = self.head / name
                original = path.read_bytes()
                path.unlink()
                with self.assertRaisesRegex(ValueError, "missing file"):
                    self.load(allow_incomplete=True)
                path.write_bytes(original)

    def test_one_changed_gpu_alpha_byte_fails_even_with_updated_output_hash(self):
        output = self.head / "default/gpu-decode/tile.bin"
        output.write_bytes(bytes([17, 33, 65, 254]))
        self.mutate_gpu(lambda m: next(r for r in m["rows"] if r["configuration"] == "default").update(
            sha256=readbacks.file_hash(output)))
        with self.assertRaisesRegex(ValueError, "independent literal differs"):
            self.load(allow_incomplete=True)

    def test_changed_input_expected_output_or_metadata_hash_fails(self):
        for suffix, error in [(".bin", "file hash"), (".input.bin", "input_sha256"),
                              (".expected.bin", "independent literal"), (".json", "metadata hash")]:
            with self.subTest(suffix=suffix):
                path = self.head / "default/gpu-decode" / ("tile" + suffix)
                original = path.read_bytes()
                path.write_bytes(bytes([byte ^ 1 for byte in original]))
                with self.assertRaisesRegex(ValueError, error):
                    self.load(allow_incomplete=True)
                path.write_bytes(original)

    def test_cpu_oracle_cannot_claim_gpu_execution(self):
        self.mutate_gpu(lambda m: m["rows"][0].update(role="cpu-oracle"))
        with self.assertRaisesRegex(ValueError, "provenance"):
            self.load()

    def test_missing_adapter_cannot_claim_gpu_execution(self):
        self.mutate_gpu(lambda m: m["rows"][0].update(adapter=None))
        with self.assertRaisesRegex(ValueError, "adapter"):
            self.load()

    def test_required_configurations_cannot_be_downgraded(self):
        self.mutate(self.head, lambda m: m["gpu_decode"].update(required_configurations=[]))
        with self.assertRaisesRegex(ValueError, "GPU decode requirement"):
            self.load()

    def test_profile_cannot_be_downgraded_or_swapped_between_backends(self):
        for profile in ["full", "empty", ""]:
            with self.subTest(profile=profile):
                self.mutate(self.head, lambda m: m["gpu_decode"].update(profile=profile))
                with self.assertRaisesRegex(ValueError, "GPU decode profile"):
                    self.load()
        self.mutate(self.head, lambda m: m["gpu_decode"].update(profile="warp"))
        self.mutate(self.head, lambda m: m["runtime"].update(os="Darwin"))
        with self.assertRaisesRegex(ValueError, "GPU decode profile"):
            self.load()

    def test_warp_inventory_is_fixed_and_preserves_all_comparison_rows(self):
        inventory = readbacks.gpu_decode_inventory("warp")
        rows = readbacks.gpu_decode_rows(inventory, ci.COMMANDS)
        self.assertEqual(len(rows), 297)
        self.assertEqual(len(readbacks.expected_rows(ci.INVENTORY)), 171)
        full = readbacks.gpu_decode_rows(readbacks.gpu_decode_inventory(), ci.COMMANDS)
        self.assertTrue(rows.items() <= full.items())
        with self.assertRaisesRegex(ValueError, "GPU decode profile"):
            readbacks.gpu_decode_inventory("empty")

    def test_gate_can_require_supplement_independently_of_artifact_declaration(self):
        self.mutate(self.head, lambda m: m.pop("gpu_decode"))
        with self.assertRaisesRegex(ValueError, "GPU decode declaration"):
            self.load(require_gpu_decode=True)

    def test_source_capability_comes_from_tested_checkout_not_overlay_harness(self):
        old = self.root / "old-checkout"
        old.mkdir()
        self.assertEqual(ci.gpu_decode_requirement(old)["required_configurations"], [])
        self.assertEqual(ci.gpu_decode_requirement(self.checkout)["required_configurations"],
                         self.inventory["configurations"])

    def test_corrupted_source_test_is_rejected(self):
        (self.head / "gpu-decode-source.rs").write_text("different source")
        with self.assertRaisesRegex(ValueError, "GPU decode source"):
            self.load()

    def test_collector_keeps_missing_expected_file_as_error(self):
        (self.head / "default/gpu-decode/tile.expected.bin").unlink()
        manifest = self.manifest(self.head)
        ci.save_gpu_decode_manifest(self.head, manifest, self.gpu_inventory)
        self.write(self.head / "manifest.json", manifest)
        report = self.compare_gpu(allow_incomplete=True)
        self.assertFalse(report["passed"])
        self.assertIn("tile.expected.bin", report["gpu_decode"]["candidate"]["errors"][0])

    def test_frozen_inventory_requires_1909_rows_in_each_configuration(self):
        expected = readbacks.gpu_decode_rows(readbacks.gpu_decode_inventory(), ci.COMMANDS)
        self.assertEqual(len(expected), 5727)
        for config in ci.COMMANDS:
            self.assertEqual(sum(key[0] == config for key in expected), 1909)

    def test_base_without_gpu_test_declares_zero_rows_with_current_harness(self):
        manifest = self.manifest(self.base)
        manifest["gpu_decode"] = ci.gpu_decode_requirement(self.root / "old-checkout", system="Windows")
        ci.save_gpu_decode_manifest(self.base, manifest, self.gpu_inventory)
        self.write(self.base / "manifest.json", manifest)
        result, _ = readbacks.load_capture(self.base, self.inventory, BASE, "10",
                                          supplemental_inventory=self.gpu_inventory, require_gpu_decode=False)
        self.assertEqual(result["_gpu_decode"]["rows"], 0)
        self.assertEqual(result["_gpu_decode"]["required_configurations"], [])
        self.assertTrue(self.compare_gpu()["passed"])

    def test_current_harness_cannot_omit_supplemental_declaration(self):
        self.mutate(self.head, lambda m: m.update(harness_files={"tools/tmem/gpu-decode-inventory.json": "f" * 64}))
        self.mutate(self.head, lambda m: m.pop("gpu_decode"))
        with self.assertRaisesRegex(ValueError, "GPU decode declaration"):
            self.load()

    def test_supplement_manifest_hash_is_authenticated(self):
        path = self.head / "gpu-decode-manifest.json"
        path.write_bytes(path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "GPU decode manifest hash"):
            self.load()

    def test_unknown_or_duplicate_rows_cannot_replace_required_inventory(self):
        path = self.head / "gpu-decode-manifest.json"
        original = path.read_bytes()
        for change, error in [(lambda m: m["rows"].append(m["rows"][0]), "duplicate row"),
                              (lambda m: m["rows"][0].update(id="unregistered"), "unexpected row"),
                              (lambda m: m["rows"][0].update(width=2), "incompatible width")]:
            with self.subTest(error=error):
                path.write_bytes(original)
                self.mutate_gpu(change)
                with self.assertRaisesRegex(ValueError, error):
                    self.load()


if __name__ == "__main__":
    unittest.main()
