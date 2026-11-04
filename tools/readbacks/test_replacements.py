import hashlib
import json
import shutil
import unittest

import readbacks
import replacements
from test_readbacks import CaptureCase


class ReplacementReadbacks(CaptureCase):
    def setUp(self):
        super().setUp()
        self.checkout = self.root / "checkout"
        source = self.checkout / replacements.SOURCE
        source.parent.mkdir(parents=True)
        source.write_text("independent replacement test")
        shutil.copyfile(source, self.head / "replacement-source.rs")
        self.frozen = {"version": 1, "rows": [{"id": "tile", "width": 1, "height": 1}]}
        directory = self.head / "all-features/replacements"
        directory.mkdir(parents=True)
        blobs = {"expected": bytes([17, 33, 65, 71]), "before": bytes([1, 2, 3, 255]), "mask": b"\1"}
        for kind, value in blobs.items():
            (directory / f"tile.{kind}.bin").write_bytes(value)
            self.frozen["rows"][0][kind + "_sha256"] = hashlib.sha256(value).hexdigest()
        (directory / "tile.bin").write_bytes(blobs["expected"])
        (directory / "tile.input.bin").write_bytes(b"owned fixture and registration")
        self.manifest = json.loads((self.head / "manifest.json").read_text())
        metadata = {"id": "tile", "width": 1, "height": 1, "channels": "RGBA", "stage": "final",
                    "role": "replacement", "blend_path": "none", "test_id": "replacement",
                    "adapter": self.manifest["rows"][0]["adapter"]}
        (directory / "tile.json").write_text(json.dumps(metadata))
        self.manifest["replacements"] = replacements.requirement(self.checkout)
        replacements.save(self.head, self.manifest, self.frozen)

    def load(self, **kwargs):
        return replacements.load(self.head, self.manifest, frozen=self.frozen, **kwargs)

    def change_supplement(self, change):
        path = self.head / "replacement-manifest.json"
        supplement = json.loads(path.read_text())
        change(supplement)
        path.write_text(json.dumps(supplement))
        self.manifest["replacements"]["manifest_sha256"] = readbacks.file_hash(path)

    def test_complete_enabled_artifact_and_changed_region(self):
        result = self.load(require=True)
        self.assertEqual(result["rows"], 1)
        self.assertEqual(result["regions"][0]["changed_pixels"], 1)
        self.assertEqual(result["regions"][0]["bounds"], [0, 0, 0, 0])

    def test_exact_parent_without_provider_requires_zero_rows(self):
        manifest = dict(self.manifest, replacements={"version": 1, "source_test_sha256": None,
                                                     "required_configurations": []})
        replacements.save(self.base, manifest, self.frozen)
        self.assertEqual(replacements.load(self.base, manifest, frozen=self.frozen)["rows"], 0)
        with self.assertRaisesRegex(ValueError, "requested checkout"):
            replacements.load(self.base, manifest, frozen=self.frozen, require=True)

    def test_missing_row_and_failure_on_partial_capture(self):
        self.change_supplement(lambda m: m["rows"].clear())
        with self.assertRaisesRegex(ValueError, "missing replacement rows"):
            self.load()
        self.assertEqual(len(self.load(allow_incomplete=True)["missing_rows"]), 1)

    def test_alpha_corruption_is_not_hidden_by_updating_file_hash(self):
        path = self.head / "all-features/replacements/tile.bin"
        path.write_bytes(bytes([17, 33, 65, 72]))
        self.change_supplement(lambda m: m["rows"][0].update(sha256=readbacks.file_hash(path)))
        with self.assertRaisesRegex(ValueError, "RGBA differs from literal"):
            self.load()

    def test_changed_expected_and_mask_cannot_rewrite_the_frozen_inventory(self):
        for kind in ["expected", "before", "mask"]:
            with self.subTest(kind=kind):
                path = self.head / f"all-features/replacements/tile.{kind}.bin"
                original = path.read_bytes()
                path.write_bytes(b"\0" * len(original))
                self.change_supplement(lambda m: m["rows"][0].update({kind + "_sha256": readbacks.file_hash(path)}))
                with self.assertRaisesRegex(ValueError, "frozen replacement"):
                    self.load()
                path.write_bytes(original)
                self.change_supplement(lambda m: m["rows"][0].update({kind + "_sha256": readbacks.file_hash(path)}))

    def test_adapter_source_input_and_manifest_are_authenticated(self):
        self.change_supplement(lambda m: m["rows"][0]["adapter"].update(device_type="DiscreteGpu"))
        with self.assertRaisesRegex(ValueError, "WARP requires"):
            self.load()

    def test_omitted_declaration_cannot_pass_a_required_capture(self):
        del self.manifest["replacements"]
        with self.assertRaisesRegex(ValueError, "missing replacement declaration"):
            self.load(require=True)
        self.manifest["harness_files"] = {replacements.INVENTORY: "f" * 64}
        with self.assertRaisesRegex(ValueError, "missing replacement declaration"):
            self.load()

    def test_same_size_but_wrong_change_mask_fails(self):
        path = self.head / "all-features/replacements/tile.mask.bin"
        path.write_bytes(b"\0")
        digest = readbacks.file_hash(path)
        self.frozen["rows"][0]["mask_sha256"] = digest
        self.manifest["replacements"]["inventory_sha256"] = readbacks.json_hash(self.frozen)
        self.change_supplement(lambda m: (m.update(inventory_sha256=readbacks.json_hash(self.frozen)),
                                         m["rows"][0].update(mask_sha256=digest)))
        with self.assertRaisesRegex(ValueError, "change mask differs"):
            self.load()
