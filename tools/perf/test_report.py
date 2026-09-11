import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

import report


class ReportTests(unittest.TestCase):
    def frame(self, serial=1, observed=True):
        return {'serial':serial, 'observed':observed, 'emission_interval_ms':2,
                'profile':{'timings':{key:{'inclusive_ms':.1,'exclusive_ms':.1}
                                      for key in ['begin_frame','process_dl','presentation']}}}

    def test_missing_flags_and_partial_timing_cannot_be_summarized_as_coarse(self):
        for defect in ['flags','partial','empty']:
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                frames = [self.frame(1), self.frame(2)]
                if defect == 'flags':
                    for frame in frames:
                        del frame['observed']
                if defect == 'partial':
                    frames[1]['profile']['timings'] = {}
                if defect == 'empty':
                    frames = []
                (root/'frames').write_text(''.join(json.dumps(f)+'\n' for f in frames))
                with self.assertRaises(ValueError):
                    report.summarize(SimpleNamespace(input=root/'frames', out=root/'summary',
                                                     configuration='coarse', expected_observed=2))
                self.assertFalse(json.loads((root/'summary').read_text())['validation']['valid'])

    def test_batch_requires_every_cell_and_handles_unlaunched_attempts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frames = [self.frame(1,False),self.frame(2),self.frame(3)]
            (root/'frames').write_text(''.join(json.dumps(f)+'\n' for f in frames))
            report.summarize(SimpleNamespace(input=root/'frames',out=root/'summary',configuration='coarse'))
            entries = []
            for i in range(5):
                order = ['parent','candidate'] if i%2==0 else ['candidate','parent']
                for j,revision in enumerate(order):
                    path = root/f'{i}-{revision}'
                    path.write_text(json.dumps({'quiet_protocol_version':report.VERSION,'valid':True,'reasons':[],'pair':i,'revision':revision,
                                               'workload':'authored','configuration':'coarse',
                                               'actual_utc_interval':[f'{i}{j}',f'{i}{j}']}))
                    entries.append({'quiet_run':str(path),'summary':str(root/'summary'),'expected_observed':2})
            plan = {'attempts':entries,'cells':['parent.authored.coarse','candidate.authored.coarse']}
            def batch():
                (root/'plan').write_text(json.dumps(plan))
                report.batch_report(SimpleNamespace(input=root/'plan',out=root/'batch'))
                return json.loads((root/'batch').read_text())
            self.assertTrue(batch()['valid'])
            plan['cells'].append('candidate.missing.coarse')
            self.assertFalse(batch()['valid'])
            plan['cells'].pop()
            path = Path(entries[0]['quiet_run'])
            run = json.loads(path.read_text())
            run.update(valid=False, reasons=['preflight rejected'], actual_utc_interval=[None,None],
                       monitoring_utc_interval=['00','01'])
            path.write_text(json.dumps(run))
            self.assertFalse(batch()['valid'])
            run.update(valid=True,reasons=[],actual_utc_interval=['00','01'])
            path.write_text(json.dumps(run))
            summary = json.loads((root/'summary').read_text())
            summary['observed'].update(frames=0,cpu_ms=None)
            (root/'summary').write_text(json.dumps(summary))
            result = batch()
            self.assertFalse(result['valid'])
            self.assertIn('missing observed CPU timing',result['attempts'][0]['reasons'])


if __name__ == '__main__':
    unittest.main()
