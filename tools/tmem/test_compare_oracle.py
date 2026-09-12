import hashlib
import unittest

from compare_oracle import compare_bytes


class ExactAnchor(unittest.TestCase):
    def setUp(self):
        self.pixels = bytes([17, 43, 89, 211])
        self.row = {'scene': 'test', 'rgba_bytes': 4, 'channels': 'RGB',
                    'fast3d_sha256': hashlib.sha256(self.pixels).hexdigest(),
                    'rt64_sha256': hashlib.sha256(self.pixels).hexdigest()}

    def test_unchanged_anchor_passes(self):
        self.assertEqual(compare_bytes(self.row, self.pixels, self.pixels, self.pixels, bytes(6))
                         ['parent_candidate_changed_bytes'], 0)

    def test_both_sides_changed_alpha_cannot_hide_behind_rgb_policy(self):
        changed = self.pixels[:3] + bytes([210])
        with self.assertRaisesRegex(ValueError, 'parent RGBA'):
            compare_bytes(self.row, changed, changed, self.pixels, bytes(6))

    def test_candidate_alpha_fails(self):
        with self.assertRaisesRegex(ValueError, 'parent/candidate RGBA'):
            compare_bytes(self.row, self.pixels, self.pixels[:3] + bytes([210]), self.pixels, bytes(6))

    def test_missing_row_fails(self):
        with self.assertRaisesRegex(ValueError, 'missing or truncated'):
            compare_bytes(self.row, self.pixels, b'', self.pixels, bytes(6))

    def test_changed_reference_fails(self):
        with self.assertRaisesRegex(ValueError, 'reference readback'):
            compare_bytes(self.row, self.pixels, self.pixels, bytes(4), bytes(6))


if __name__ == '__main__':
    unittest.main()
