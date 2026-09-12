import gzip
import hashlib
import json
from pathlib import Path
import struct
import unittest

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


class FrozenBaselines(unittest.TestCase):
    def test_all_27_goldens_match_frozen_c2_readbacks(self):
        ledger = json.loads((HERE / 'goldens-bb0fea3.json').read_text())
        rows = ledger['goldens']
        self.assertEqual(len(rows), 27)
        self.assertEqual({row['file'] for row in rows},
                         {str(path.relative_to(ROOT)) for path in (ROOT / 'fast3d/goldens').glob('*.bin')})
        for row in rows:
            with self.subTest(file=row['file']):
                data = (ROOT / row['file']).read_bytes()
                self.assertEqual(len(data), row['width'] * row['height'] * 4)
                self.assertEqual(sha(data), row['sha256'])
                self.assertEqual(set(row['c2_readbacks']), {'default', 'debug-ui', 'all-features'})
                self.assertEqual(set(row['c2_readbacks'].values()), {row['sha256']})

    def test_oracle_residual_values_and_exact_masks_are_intact(self):
        ledger = json.loads((HERE / 'oracle-bb0fea3.json').read_text())
        self.assertEqual(len(ledger['rows']), 60)
        self.assertEqual(len({row['scene'] for row in ledger['rows']}), 60)
        self.assertEqual(sum(row['scene'].startswith('coverage-') for row in ledger['rows']), 38)
        for row in ledger['rows']:
            with self.subTest(scene=row['scene']):
                data = gzip.decompress((HERE / row['residual_file']).read_bytes())
                self.assertEqual(sha(data), row['residual_sha256'])
                channels = len(row['channels'])
                self.assertEqual(len(data), row['rgba_bytes'] // 4 * channels * 2)
                values = [v[0] for v in struct.iter_unpack('<h', data)]
                self.assertTrue(all(-255 <= value <= 255 for value in values))
                mask = bytes(any(values[i:i + channels]) for i in range(0, len(values), channels))
                self.assertEqual(sha(mask), row['mask_sha256'])
                self.assertEqual(sum(mask), row['changed_pixels'])
                self.assertEqual(max(map(abs, values)), row['maximum'])
                if 'limit' in row:
                    self.assertLessEqual(row['maximum'], row['limit'])
        quad = next(row for row in ledger['rows'] if row['scene'] == 'f3dex2-quad-winding')
        self.assertEqual((quad['maximum'], quad['changed_pixels']), (0, 0))
        self.assertEqual(len(ledger['controls']), 1)
        tri2 = ledger['controls'][0]
        self.assertEqual(tri2['scene'], 'f3dex2-quad-winding-tri2')
        self.assertEqual(tri2['equivalent_to'], quad['scene'])
        self.assertEqual(tri2['channels'], 'RGBA')
        self.assertEqual((tri2['maximum'], tri2['changed_pixels']), (0, 0))
        self.assertEqual(tri2['rt64_sha256'], quad['rt64_sha256'])

    def test_role_count_expectations_describe_fixture_requests(self):
        contract = json.loads((HERE / 'dispatch-expectations.json').read_text())
        roles = contract['roles_fixture']
        distinct = set(role for draw in roles['draw_roles'] for role in draw)
        self.assertEqual(distinct, {'r0', 'r1', 'r2', 'r3'})
        self.assertEqual(roles['cold_dispatches'], len(distinct))
        self.assertTrue(set(roles['level_zero_aliases']) <= distinct)
        self.assertEqual(roles['warm_dispatches'], 0)
        for row in contract['changes']:
            self.assertEqual(row['dispatches'], len(row['new_requests']))


if __name__ == '__main__':
    unittest.main()
