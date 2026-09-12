import json
import tempfile
import unittest
from pathlib import Path

from ledger import compare_ledger, sha
from predict import mask_file


class GoldenLedger(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.before = self.root / "before.bin"
        self.before.write_bytes(bytes([10, 20, 30, 255] * 3))
        self.after = self.root / "sample.bin"
        self.values = self.root / "values.rgba8"
        self.values.write_bytes(bytes([10, 20, 30, 255, 90, 80, 70, 128, 10, 20, 30, 255]))
        mask = self.root / "mask.rgba8"
        prediction = {"file": mask.name, **mask_file(mask, [False, True, False], 3)}
        self.frozen = self.root / "ledger.json"
        self.frozen.write_text(json.dumps({"goldens": [{
            "file": "fast3d/goldens/sample.bin", "width": 3, "height": 1,
            "before_file": self.before.name, "before_sha256": sha(self.before),
            "prediction": prediction,
            "predicted_values": {"file": self.values.name, "sha256": sha(self.values), "channel_tolerance": 2},
        }]}))

    def run_comparison(self, data):
        self.after.write_bytes(data)
        report = compare_ledger(self.frozen, self.root, self.root / "result")
        return report["passed"], report["goldens"][0]["candidate"]

    def test_exact_mask_and_values_pass(self):
        passed, row = self.run_comparison(self.values.read_bytes())
        self.assertTrue(passed)
        self.assertEqual(row["byte-delta"]["pixels"], 1)
        self.assertEqual(row["byte-delta"]["bounds_inclusive"], [1, 0, 1, 0])
        self.assertIsNone(row["missing"]["bounds_inclusive"])

    def test_alpha_byte_outside_mask_is_held_below_tolerance(self):
        data = bytearray(self.values.read_bytes())
        data[3] -= 1
        passed, row = self.run_comparison(data)
        self.assertFalse(passed)
        self.assertEqual(row["outside"]["pixels"], 1)
        self.assertEqual(row["byte-delta"]["pixels"], 2)
        self.assertEqual(row["over-two"]["pixels"], 1)

    def test_missing_prediction_is_held(self):
        passed, row = self.run_comparison(self.before.read_bytes())
        self.assertFalse(passed)
        self.assertEqual(row["missing"]["pixels"], 1)

    def test_right_mask_with_wrong_values_is_held(self):
        data = bytearray(self.values.read_bytes())
        data[7] += 3
        passed, row = self.run_comparison(data)
        self.assertFalse(passed)
        self.assertEqual(row["wrong-values"]["pixels"], 1)
        self.assertEqual(row["outside"]["pixels"], 0)

    def test_value_rounding_does_not_relax_mask_membership(self):
        data = bytearray(self.values.read_bytes())
        data[7] += 2
        self.assertTrue(self.run_comparison(data)[0])

    def test_changed_frozen_mask_is_rejected(self):
        (self.root / "mask.rgba8").write_bytes(bytes(12))
        with self.assertRaises(AssertionError):
            self.run_comparison(self.values.read_bytes())


if __name__ == "__main__":
    unittest.main()
