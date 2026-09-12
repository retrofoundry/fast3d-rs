import unittest

from compare_three_way import compare


class OracleGate(unittest.TestCase):
    def test_coverage_improvement_cannot_hide_new_shading_error(self):
        oracle = bytes([0, 0, 0, 255] * 2)
        parent = bytes([6, 0, 0, 255, 0, 0, 0, 255])
        candidate = bytes([0, 0, 0, 255, 5, 0, 0, 255])
        coverage = bytes([255, 255, 255, 255, 0, 0, 0, 255])
        _, unclassified, limit, passed = compare(parent, candidate, oracle, 3, 6, [coverage])
        self.assertEqual(limit, 6)
        self.assertEqual(unclassified, [False, True])
        self.assertFalse(passed)

    def test_measured_zero_is_not_a_threshold_budget(self):
        parent = bytes([0, 0, 0, 255])
        candidate = bytes([1, 0, 0, 255])
        _, _, limit, passed = compare(parent, candidate, parent, 3, 7, [bytes([255] * 4)])
        self.assertEqual(limit, 0)
        self.assertFalse(passed)

    def test_preserves_alpha_policy(self):
        parent = bytes([0, 0, 0, 255])
        oracle = bytes([0, 0, 0, 0])
        self.assertTrue(compare(parent, parent, oracle, 3, 0, [])[3])
        self.assertFalse(compare(parent, parent, oracle, 4, 0, [])[3])

    def test_parent_candidate_alpha_change_needs_explanation(self):
        parent = bytes([0, 0, 0, 255])
        candidate = bytes([0, 0, 0, 128])
        self.assertFalse(compare(parent, candidate, parent, 3, 0, [])[3])


if __name__ == "__main__":
    unittest.main()
