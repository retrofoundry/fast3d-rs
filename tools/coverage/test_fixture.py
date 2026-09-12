import tempfile
import unittest
from pathlib import Path

from check_fixture import check


class F3dOracleQuirk(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.scene = "coverage-slopes-f3d"
        self.expected = bytearray(bytes([0, 0, 0, 255]) * (320 * 240))
        self.missing = []
        for origin, spans in [
            (8, [(8, 24), (8, 20), (8, 16), (8, 12)]),
            (52, [(8, 24), (12, 24), (16, 24), (20, 24)]),
            (97, [(8, 12), (8, 16), (8, 20)]),
            (141, [(20, 24), (16, 24), (12, 24)]),
        ]:
            for winding in [0, 20]:
                for row, (left, right) in enumerate(spans):
                    for x in range(left, right):
                        i = ((origin + winding + row) * 320 + x) * 4
                        self.expected[i:i + 3] = bytes([255, 0, 0])
                        self.missing.append(i)
        self.actual = self.expected.copy()
        for i in self.missing:
            self.actual[i:i + 3] = bytes(3)
        self.write_inputs()

    def write_inputs(self):
        (self.directory / "coverage-manifest.tsv").write_text(
            "scene\twidth\theight\tthreshold\n"
            f"{self.scene}\t320\t240\t0\n"
        )
        for rule in ["a", "b"]:
            (self.directory / f"{self.scene}.expected-{rule}.rgba8").write_bytes(self.expected)

    def result(self, actual=None, rt64=True, quirk=True):
        return check(self.directory, self.scene, self.actual if actual is None else actual,
                     rt64, allow_rt64_f3d_quirk=quirk)

    def test_strict_gate_still_reports_all_missing_pixels(self):
        result = self.result(quirk=False)
        self.assertFalse(result["passed"])
        self.assertEqual(result["mismatched_pixels"], 256)

    def test_named_quirk_preserves_the_raw_discrepancy(self):
        result = self.result()
        self.assertTrue(result["passed"])
        self.assertEqual(result["mismatched_pixels"], 256)
        self.assertEqual(result["unclassified_pixels"], 0)

    def test_quirk_rejects_any_new_or_recovered_pixel(self):
        for i in [self.missing[0], (8 * 320 + 24) * 4, (12 * 320 + 8) * 4, 0]:
            with self.subTest(pixel=i // 4):
                actual = self.actual.copy()
                actual[i] = 1
                self.assertFalse(self.result(actual)["passed"])
        self.assertFalse(self.result(self.expected)["passed"])

    def test_quirk_cannot_change_its_shape(self):
        self.expected[self.missing[0]] = 0
        self.write_inputs()
        with self.assertRaises(ValueError):
            self.result()

    def test_quirk_is_only_available_for_rt64_f3d_slopes(self):
        with self.assertRaises(ValueError):
            self.result(rt64=False)
        self.scene = "coverage-slopes-f3dex2"
        self.write_inputs()
        with self.assertRaises(ValueError):
            self.result()

    def test_quirk_rejects_statistical_policy_before_checking_pixels(self):
        path = self.directory / "coverage-manifest.tsv"
        path.write_text(path.read_text().replace("\t0\n", "\tstatistical\n"))
        with self.assertRaises(ValueError):
            self.result()


if __name__ == "__main__":
    unittest.main()
