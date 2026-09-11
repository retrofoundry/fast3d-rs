import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(os.environ.get('B2_CANDIDATE_BIN') and os.environ.get('B2_PARENT_BIN'),
                     'set B2_CANDIDATE_BIN and B2_PARENT_BIN for the native GPU regression')
class NativeTests(unittest.TestCase):
    def test_authored_sequence_reaches_summary_in_both_modes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for revision in ['candidate', 'parent']:
                binary = os.environ[f'B2_{revision.upper()}_BIN']
                for mode in ['coarse', 'counters']:
                    out = root/f'{revision}-{mode}'
                    subprocess.run([binary, 'sequence', 'authored', str(out), mode] + (['--parent-compatible'] if revision=='candidate' else []), check=True)
                    subprocess.run([sys.executable, str(Path(__file__).with_name('report.py')),
                                    'summarize', '--input', str(out/'frames.jsonl'),
                                    '--out', str(out/'summary.json'), '--configuration', mode,
                                    '--expected-observed', '4'], check=True)
                    frames = [json.loads(line) for line in (out/'frames.jsonl').read_text().splitlines()]
                    summary = json.loads((out/'summary.json').read_text())
                    self.assertEqual(len(frames), 6)
                    self.assertEqual([f['serial'] for f in frames if f['observed']], [3,4,5,6])
                    self.assertTrue(all(f['rgba8_sha256'] is None for f in frames))
                    self.assertTrue(summary['validation']['valid'])
                    self.assertEqual(summary['observed']['frames'], 4)
                    if mode == 'coarse':
                        self.assertGreater(summary['observed']['cpu_ms'], 0)
                    else:
                        self.assertIsNone(summary['observed']['cpu_ms'])
                        if revision == 'candidate':
                            self.assertTrue(summary['observed']['counters'])
                    self.assertGreater(summary['observed']['emission_interval_ms'], 0)
            reference = root/'ordinary'
            subprocess.run([os.environ['B2_CANDIDATE_BIN'], 'ordinary', 'authored', str(reference)], check=True)
            for revision in ['candidate','parent']:
                out = root/f'{revision}-readback'
                subprocess.run([os.environ[f'B2_{revision.upper()}_BIN'], 'sequence', 'authored', str(out),
                                'coarse', '--readback'], check=True)
                subprocess.run([sys.executable, str(Path(__file__).with_name('report.py')), 'compare',
                                '--input', str(reference/'frames.jsonl'), '--other', str(out/'frames.jsonl'),
                                '--out', str(out/'comparison.json')], check=True)


if __name__ == '__main__':
    unittest.main()
