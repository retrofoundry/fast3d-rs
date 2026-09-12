import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


class T1Tests(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).with_name('t1.py')
        self.assertTrue(path.exists(), 'T1 executable kit is missing')
        spec = importlib.util.spec_from_file_location('t1', path)
        self.kit = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.kit)

    def test_plan_reuses_one_binary_and_preserves_alternation(self):
        plan = self.kit.native_plan(Path('/relay with spaces'), Path('/attempt'))
        self.assertEqual(len(plan['attempts']), 10)
        self.assertEqual([a['revision'] for a in plan['attempts']],
                         ['parent', 'candidate', 'candidate', 'parent', 'parent',
                          'candidate', 'candidate', 'parent', 'parent', 'candidate'])
        for attempt in plan['attempts']:
            self.assertEqual(attempt['command'][:3],
                             ['/relay with spaces/bin/b1-perf', 'sequence',
                              '/relay with spaces/captures/demo1-dense.f3dcap'])
            self.assertEqual(attempt['command'][4:], ['coarse'])
            self.assertEqual(attempt['expected_observed'], 120)

    def test_identical_binary_directional_bias_rejects_stable_spreads(self):
        self.assertFalse(self.kit.metric_verdict([100]*5, [102]*5, .5)['valid'])
        self.assertFalse(self.kit.metric_verdict([102]*5, [100]*5, .5)['valid'])
        self.assertTrue(self.kit.metric_verdict([100]*5, [100.2]*5, .5)['valid'])
        self.assertTrue(self.kit.metric_verdict([100]*5, [99,101,99,101,100], .5)['valid'])

    def test_failed_or_nonfinite_repetitions_never_pass(self):
        for values in [[100]*4, [100]*4+[float('nan')], [100]*4+[float('inf')],
                       [100]*4+[106], [100]*4+[0]]:
            with self.subTest(values=values):
                self.assertFalse(self.kit.metric_verdict(values, [100]*5, .5)['valid'])

    def test_window_limit_counts_incomplete_reservations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = ('2026-09-13T00:00:00Z', '2026-09-13T01:00:00Z')
            self.kit.reserve(root, 'first', *args, 'ci8', 'operator statement')
            self.kit.reserve(root, 'second', *args, 'ci8', 'operator statement')
            with self.assertRaises(ValueError):
                self.kit.reserve(root, 'third', *args, 'ci8', 'operator statement')
            decision = self.kit.decision(root)
            self.assertEqual(decision['status'], 'stop-protocol-decision')
            self.assertFalse(decision['valid'])

    def test_native_success_keeps_chrome_driver_admission_separate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            window = self.kit.reserve(root, 'first', '2026-09-13T00:00:00Z',
                                      '2026-09-13T01:00:00Z', 'ci8', 'operator statement')
            (window/'native-demo1-dense').mkdir()
            (window/'native-demo1-dense'/'verdict.json').write_text(json.dumps(
                {'valid': True, 'cell': 'native.demo1-dense', 'policy': 'b2-quiet-v3'}))
            result = self.kit.decision(root)
            self.assertTrue(result['valid'])
            self.assertEqual(result['status'], 'native-pilot-pass')
            self.assertEqual(result['missing_cells'], [])
            self.assertEqual(result['chrome_driver_admission'], {
                'required': True, 'host': 'ci4', 'status': 'pending-separate-cell',
                'timing_valid': False})
            self.assertFalse(result['full_t1_valid'])

    def test_native_reservation_rejects_ci4(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                self.kit.reserve(Path(directory), 'first', '2026-09-13T00:00:00Z',
                                 '2026-09-13T01:00:00Z', 'ci4', 'operator statement')

    def test_frame_admission_rejects_truncated_prefix_and_timed_readback(self):
        frames = [{'serial': i, 'observed': i >= 1400, 'rgba8_sha256': None}
                  for i in range(1, 1520)]
        self.assertEqual(self.kit.frame_errors(frames), [])
        self.assertTrue(self.kit.frame_errors(frames[1:]))
        frames[100]['rgba8_sha256'] = 'readback'
        self.assertTrue(self.kit.frame_errors(frames))

    def test_manifest_covers_full_prefix_on_both_backends(self):
        rows = self.kit.row_manifest()
        self.assertEqual(len(rows), 10)
        for backend in ['native', 'chrome']:
            selected = {r['workload']: r for r in rows if r['backend'] == backend}
            self.assertEqual({w for w, r in selected.items() if r['role'] == 'primary'},
                             {'demo1-dense', 'demo2', 'source'})
            self.assertEqual(selected['demo2']['prefix'], [1, 2719])
            self.assertEqual(selected['source']['observed'], [120, 719])
            self.assertEqual(selected['source']['warmup'], 120)
            self.assertTrue(all(r['driver_admission'] == 'blocked-until-executed' for r in selected.values()))

    def test_interrupted_attempt_has_an_inconclusive_immutable_verdict(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            plan = self.kit.native_plan(Path('/relay'), out)
            (out/'attempts.json').write_text(json.dumps(plan))
            result = self.kit.audit(out)
            self.assertFalse(result['valid'])
            self.assertEqual(result['status'], 'inconclusive')
            with self.assertRaises(FileExistsError):
                self.kit.audit(out)

    def test_missing_chrome_driver_artifacts_cannot_be_admitted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertTrue(hasattr(self.kit, 'browser_admit'), 'Chrome driver admission is missing')
            result = self.kit.browser_admit('source', root/'coarse', root/'readback',
                                             root/'one-frame', root/'admission.json')
            self.assertFalse(result['valid'])
            self.assertFalse(result['timing_valid'])

    def test_chrome_admission_requires_emission_intervals_and_complete_teardown(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for mode in ['coarse', 'readback', 'one-frame']:
                target = root/mode
                target.mkdir()
                frames = [{'serial': i, 'observed': i >= 120,
                           'rgba8_sha256': None if mode == 'coarse' else 'a'*64,
                           'emission_interval_ms': None if i == 0 else 1,
                           'profile': {'timings': {phase: {'inclusive_ms': .1, 'exclusive_ms': .1}
                                                   for phase in ['begin_frame', 'process_dl', 'presentation']}}}
                          for i in range(720)]
                result = {'setup': {'frames_in_flight': 1 if mode == 'one-frame' else 2,
                                    'adapter': 'test', 'features': 'test', 'limits': 'test'}, 'frames': frames}
                (target/'result.json').write_text(json.dumps(result))
                (target/'result.jsonl').write_text(''.join(json.dumps(f)+'\n' for f in frames))
                (target/'browser-capabilities.json').write_text('{}')
                (target/'browser-teardown.json').write_text('{"complete":true,"survivors":[]}')
            def admit(name):
                return self.kit.browser_admit('source', root/'coarse', root/'readback',
                                              root/'one-frame', root/name)
            self.assertTrue(admit('good.json')['valid'])
            source = root/'coarse/result.jsonl'
            frames = [json.loads(line) for line in source.read_text().splitlines()]
            frames[120].pop('emission_interval_ms')
            source.write_text(''.join(json.dumps(f)+'\n' for f in frames))
            self.assertFalse(admit('missing-interval.json')['valid'])
            (root/'one-frame/browser-teardown.json').write_text('{"complete":false,"survivors":[42]}')
            self.assertFalse(admit('survivor.json')['valid'])


if __name__ == '__main__':
    unittest.main()
