import copy
import gzip
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import protocol
import report
import t1


class TimingV4Tests(unittest.TestCase):
    def batch(self, root, changes=None, version=protocol.V4):
        plan = t1.native_plan(root, root, version)
        plan['floors_ms'] = {'cpu_ms': .01, 'emission_interval_ms': .01}
        for i, entry in enumerate(plan['attempts']):
            quiet = Path(entry['quiet_run'])
            quiet.parent.mkdir(parents=True, exist_ok=True)
            run = {**entry, 'quiet_protocol_version': version, 'valid': True, 'reasons': [],
                   'actual_utc_interval': [i, i+1]}
            quiet.write_text(json.dumps(run))
            summary = {'validation': {'valid': True}}
            for window, frames, intervals in [('cold', 1399, 1398), ('observed', 120, 120)]:
                values = {metric: 100 for metric in ['cpu_ms', 'emission_interval_ms']}
                if changes:
                    changes(values, window, entry, i)
                summary[window] = {'frames': frames, 'emission_intervals': intervals,
                    'library_timing_available': True, 'cpu_ms': values['cpu_ms']*frames,
                    'emission_interval_ms': values['emission_interval_ms']*intervals,
                    'assembly_ms': 1+i, 'timings': {'tiny.inclusive_ms': 1+i}}
            path = Path(entry['summary'])
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(summary))
        return plan

    def verdict(self, root, plan):
        (root/'plan.json').write_text(json.dumps(plan))
        report.batch_report(SimpleNamespace(input=root/'plan.json', out=root/'batch.json'))
        return json.loads((root/'batch.json').read_text())

    def test_at_and_over_each_per_revision_aggregate_limit(self):
        for window in ['cold', 'observed']:
            for metric in ['cpu_ms', 'emission_interval_ms']:
                limit = .05 if window == 'cold' else .25 if metric == 'cpu_ms' else .15
                for revision in ['parent', 'candidate']:
                    for excess in [0, .000001]:
                        with self.subTest(window=window, metric=metric, revision=revision, excess=excess), tempfile.TemporaryDirectory() as directory:
                            root = Path(directory)
                            def change(values, selected, entry, i):
                                if selected == window and entry['revision'] == revision and entry['pair'].endswith('-5'):
                                    values[metric] += 100*limit+excess
                            result = self.verdict(root, self.batch(root, change))
                            self.assertEqual(result['valid'], excess == 0)
                            self.assertEqual(result['policy'], protocol.V4)
                            self.assertAlmostEqual(result['lost_sensitivity'][f'{window}.{metric}']['ms_per_frame'], 100*limit+excess)

    def test_components_are_diagnostic_only_in_v4(self):
        for version in [protocol.VERSION, protocol.V4]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                result = self.verdict(root, self.batch(root, version=version))
                self.assertEqual(result['valid'], version == protocol.V4)
                self.assertTrue(result['lost_sensitivity'])

    def test_scope_rejects_other_rows_and_configurations(self):
        for backend, workload, configuration in [('chrome','demo1-dense','coarse'),
                ('native','demo2','coarse'), ('native','demo1-dense','counters'),
                (None,'demo1-dense','coarse')]:
            with self.assertRaises(ValueError):
                protocol.timing_policy(protocol.V4, backend, workload, configuration)
            self.assertEqual(protocol.timing_policy(protocol.VERSION, backend, workload, configuration), protocol.POLICY)
        rows = t1.row_manifest()
        self.assertEqual([r['id'] for r in rows if r['policies']['coarse'] == protocol.V4], ['native.demo1-dense'])
        self.assertTrue(all(r['policies']['counters'] == protocol.VERSION for r in rows))

    def test_v3_sensitivity_does_not_mix_workloads(self):
        runs, summaries = [], {}
        for workload, value in [('demo2', 1), ('source', 100)]:
            for revision in ['parent','candidate']:
                for i in range(5):
                    name = f'{workload}-{revision}-{i}'
                    runs.append({'backend':'native', 'workload':workload, 'configuration':'coarse',
                                 'revision':revision, 'artifacts':{'summary':name}})
                    summaries[name] = {window:{'cpu_ms':value*120, 'frames':120,
                        'emission_interval_ms':value*120, 'emission_intervals':120}
                        for window in ['cold','observed']}
        metrics = report.batch_metrics({}, runs, summaries, protocol.VERSION)
        self.assertEqual(len(metrics), 8)
        self.assertTrue(all(m['lost_sensitivity']['ms_per_frame'] == .01 for m in metrics.values()))

    def test_missing_rejected_duplicate_extra_and_wrong_order_pairs(self):
        for defect in ['unlaunched', 'missing', 'rejected', 'duplicate', 'extra', 'order', 'version', 'floor', 'cold']:
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                plan = self.batch(root)
                first = plan['attempts'][0]
                path = Path(first['quiet_run'])
                run = json.loads(path.read_text())
                if defect == 'unlaunched':
                    path.unlink()
                elif defect == 'missing':
                    plan['attempts'].pop(0)
                elif defect == 'rejected':
                    run.update(valid=False, reasons=['postflight idle failure'])
                    path.write_text(json.dumps(run))
                elif defect == 'duplicate':
                    plan['attempts'][1] = first
                elif defect == 'extra':
                    plan['attempts'].append(first)
                elif defect == 'order':
                    run['actual_utc_interval'] = [2.5, 2.6]
                    path.write_text(json.dumps(run))
                elif defect == 'version':
                    run['quiet_protocol_version'] = protocol.VERSION
                    path.write_text(json.dumps(run))
                elif defect == 'floor':
                    plan['floors_ms']['cpu_ms'] = .02
                elif defect == 'cold':
                    path = Path(first['summary'])
                    summary = json.loads(path.read_text())
                    summary['cold']['emission_intervals'] = 1399
                    path.write_text(json.dumps(summary))
                result = self.verdict(root, plan)
                self.assertFalse(result['valid'])
                self.assertTrue(result['lost_sensitivity'])

    def test_directional_rule_at_and_above_uncertainty(self):
        for version in [protocol.VERSION, protocol.V4]:
            for floor, delta in [(.01, .01), (.5, .5)]:
                for excess in [0, .000001]:
                    for sign in [-1, 1]:
                        with self.subTest(version=version, floor=floor, excess=excess, sign=sign):
                            a = [100]*5
                            b = [100+sign*(delta+excess)]*5
                            result = report.metric_verdict(a, b, floor, version, identical=True)
                            self.assertEqual(result['valid'], excess == 0)
        a = [99, 100, 100, 100, 101]
        for excess in [0, .000001]:
            b = [v+2+excess for v in a]
            self.assertEqual(report.metric_verdict(a, b, .01, protocol.V4)['valid'], excess == 0)
            for parent, candidate in [([100]*5, [102,103,104,104,106]),
                                      ([98,100,100,100,102], [104]*5)]:
                candidate = [v+excess for v in candidate]
                self.assertEqual(report.metric_verdict(parent, candidate, .01, protocol.V4)['valid'], excess == 0)
        result = report.metric_verdict([100]*5, [99]*5, .01, protocol.V4)
        self.assertTrue(result['valid'])
        self.assertTrue(result['resolved_speedup'])
        self.assertFalse(report.metric_verdict([100]*5, [101]*5, .01, protocol.V4)['valid'])
        self.assertTrue(report.metric_verdict([100]*5, [101,101,101,101,100], .01, protocol.V4)['valid'])

    def test_normalization_and_cold_directional_gate(self):
        summary = {'cold': {'frames': 1399, 'emission_intervals': 1398,
                            'cpu_ms': 1399, 'emission_interval_ms': 1398}}
        self.assertEqual(report.normalized_metric(summary, 'cold', 'cpu_ms'), 1)
        self.assertEqual(report.normalized_metric(summary, 'cold', 'emission_interval_ms'), 1)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def change(values, window, entry, i):
                if window == 'cold' and entry['revision'] == 'candidate':
                    values['cpu_ms'] += .02
            result = self.verdict(root, self.batch(root, change))
            self.assertFalse(result['valid'])
            self.assertTrue(result['metrics']['cold.cpu_ms']['repeatable_regression'])
            self.assertEqual(result['metrics']['cold.cpu_ms']['effective_uncertainty_ms'], .01)

    def test_code_comparison_requires_passing_hashed_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            plan = self.batch(root)
            plan['same_binary_for_both_labels'] = False
            self.assertFalse(self.verdict(root, plan)['valid'])
            proof = root/'validation.json'
            proof.write_text(json.dumps({'valid':True, 'policy':protocol.V4,
                'cell':'native.demo1-dense', 'same_binary_for_both_labels':True}))
            plan['validation_verdict'] = {'path':str(proof), 'sha256':protocol.sha(proof)}
            self.assertTrue(self.verdict(root, plan)['valid'])
            proof.write_text(proof.read_text()+'\n')
            self.assertFalse(self.verdict(root, plan)['valid'])

    def samples(self, phase, count, start=0):
        return [{'phase':phase, 'monotonic':start+i, 'idle_percent':99, 'idle_age_seconds':.1,
                 'background_cores':.01, 'swapouts':0, 'thermal_throttled':False,
                 'power':'AC', 'power_mode':'fixed', 'processes':[]} for i in range(count)]

    def test_burst_at_and_above_limit_in_each_phase_and_across_boundary(self):
        resting = self.samples('resting', 30)
        profile = protocol.resting_profile(resting)
        for phase, count in [('preflight',30), ('run',30), ('postflight',10)]:
            for excess in [0, .000001]:
                for boundary in [False, True]:
                    with self.subTest(phase=phase, excess=excess, boundary=boundary):
                        samples = self.samples(phase, count, 30)
                        history = copy.deepcopy(resting)
                        samples[0]['background_cores'] = .75+excess
                        (history[-1] if boundary else samples[1])['background_cores'] = .75+excess
                        reasons = protocol.sample_reasons(samples, phase, profile, history=history, version=protocol.V4)
                        self.assertEqual(any('ceiling' in r for r in reasons), excess > 0)
        self.assertAlmostEqual(profile['thresholds']['maximum_background_cores_two_samples'], .06)

    def test_sustained_idle_thermal_power_and_process_limits_still_apply(self):
        profile = protocol.resting_profile(self.samples('resting', 30))
        for defect in ['sustained','idle','thermal','power','swap','gap','process']:
            samples = self.samples('postflight', 30, 30)
            if defect == 'sustained':
                # Above the fixed 0.15-core allowance David accepted on 2026-09-13; 0.11 is now
                # admitted on purpose, which FixedMeasurementThresholds covers with real evidence.
                for s in samples: s['background_cores'] = .2
            elif defect == 'idle':
                for s in samples: s['idle_percent'] = 90
            elif defect == 'thermal':
                samples[5]['thermal_throttled'] = True
            elif defect == 'power':
                samples[5]['power'] = 'battery'
            elif defect == 'swap':
                samples[5]['swapouts'] = 1
            elif defect == 'gap':
                samples[5]['monotonic'] += .8
            else:
                for s in samples: s['processes'] = [{'pid':1,'command':'Safari','cores':.04}]
            self.assertTrue(protocol.sample_reasons(samples, 'postflight', profile, version=protocol.V4), defect)

    def test_accepted_reservation_is_separate_and_consumed_even_if_interrupted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            acceptance = root/'acceptance.md'
            acceptance.write_text('David accepted the native demo1-dense v4 rule on 2026-09-13')
            (root/'validation-authorization.json').write_text(json.dumps({
                'policy':protocol.V4,'window':t1.V4_WINDOW,'accepted_by':'David',
                'prior_windows':[{'name':'window-a','policy':protocol.VERSION,'path':'/external/window-a'},
                                 {'name':'window1','policy':protocol.VERSION,'path':str(root/'window1')}],
                'acceptance':{'path':str(acceptance),'sha256':protocol.sha(acceptance)}}))
            (root/'window1').mkdir()
            args = ('2026-09-13T00:00:00Z','2026-09-13T01:00:00Z','ci8','operator statement')
            self.assertEqual(t1.decision(root)['status'], 'awaiting-v4-validation')
            with self.assertRaises(ValueError):
                t1.reserve(root, 'third-v3', *args)
            with self.assertRaises(ValueError):
                t1.reserve(root, 'third', *args, version=protocol.V4)
            window = t1.reserve(root, t1.V4_WINDOW, *args, version=protocol.V4)
            self.assertEqual(t1.decision(root)['reserved_windows'], 3)
            self.assertFalse(t1.decision(root)['valid'])
            with self.assertRaises(ValueError):
                t1.reserve(root, t1.V4_WINDOW, *args, version=protocol.V4)
            target = window/'native-demo1-dense'
            target.mkdir()
            (target/'verdict.json').write_text(json.dumps({'valid':True,'policy':protocol.V4,
                'cell':t1.NATIVE_PILOT,'lost_sensitivity':{'observed.cpu_ms':{'ms_per_frame':.2}}}))
            self.assertTrue(t1.decision(root)['valid'])
            self.assertFalse(t1.decision(root)['full_t1_valid'])

    def test_native_runs_one_full_prefix_warmup_then_fixed_settle_then_ten_processes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            window = root/t1.V4_WINDOW
            window.mkdir()
            (root/'kit.json').write_text('{}')
            (window/'reservation.json').write_text(json.dumps({'policy':protocol.V4,
                'scheduled_start':'2026-09-13T00:00:00Z', 'scheduled_end':'2099-01-01T00:00:00Z',
                'operator_attestation':'test reservation'}))
            events = []
            with patch.object(t1, 'verify_kit'), patch.object(t1, 'worker_probe', return_value={'errors':[]}), \
                    patch.object(t1.subprocess, 'run', side_effect=lambda argv, **kw: events.append(argv)), \
                    patch.object(t1.time, 'sleep', side_effect=lambda seconds: events.append(seconds)), \
                    patch.object(t1, 'audit', return_value={'valid':True}):
                self.assertEqual(t1.native(SimpleNamespace(root=root, window=window,
                    cpu_floor_ms=.01, elapsed_floor_ms=.01)), 0)
            self.assertIn('driver-checks', events[0])
            self.assertEqual(events[1][1:3], ['sequence', str(root/'captures/demo1-dense.f3dcap')])
            self.assertEqual(events[1][3:], [str(window/'native-demo1-dense/warmup'), 'coarse'])
            self.assertEqual(events[2], 90)
            self.assertEqual(len(events[3:]), 10)
            self.assertTrue(all(protocol.V4 in argv and '--backend' in argv for argv in events[3:]))


class RecordedWindowTests(unittest.TestCase):
    def test_both_recorded_windows_under_both_versions(self):
        for label in ['a', 'b']:
            path = Path(__file__).with_name('fixtures')/f'timing-window-{label}.json.gz'
            fixture = json.loads(gzip.decompress(path.read_bytes()))
            for version in [protocol.VERSION, protocol.V4]:
                with self.subTest(window=label, version=version), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    plan = t1.native_plan(root, root, version)
                    plan['floors_ms'] = {'cpu_ms':.01, 'emission_interval_ms':.01}
                    quiet_count = 0
                    original_incomplete = set()
                    for attempt in fixture['attempts']:
                        record = {**attempt['record'], 'backend':'native'}
                        samples = copy.deepcopy(attempt['samples'])
                        identities = attempt['process_identities']
                        for sample in samples:
                            sample['processes'] = [dict(zip(['pid','started','command'], identities[index]),
                                cpu_seconds=cpu, exemption=exemption, cores=None)
                                for index, cpu, exemption in sample['processes']]
                        replay = protocol.replay_attempt(samples, record, version, 'native')
                        quiet_count += replay['quiet_valid']
                        self.assertEqual(replay['quiet_protocol_version'], version)
                        self.assertFalse(replay['timing_valid'])
                        if record['exit_code'] != 0:
                            original_incomplete.add(record['pair'])
                            self.assertFalse(replay['complete'])
                            self.assertFalse(replay['quiet_valid'])
                        if label == 'b' and record['pair'].endswith('-5') and record['revision'] == 'parent':
                            self.assertFalse(replay['quiet_valid'])
                            for reason in ['ceiling','background average','idle trimmed mean']:
                                self.assertTrue(any(reason in r for r in replay['reasons']), reason)
                        entry = next(a for a in plan['attempts'] if a['pair'] == record['pair'] and a['revision'] == record['revision'])
                        target = Path(entry['quiet_run'])
                        target.parent.mkdir(parents=True)
                        target.write_text(json.dumps({**record, 'quiet_protocol_version':version,
                            'valid':replay['quiet_valid'], 'reasons':replay['reasons']}))
                        if attempt['summary']:
                            target = Path(entry['summary'])
                            target.parent.mkdir(parents=True)
                            target.write_text(json.dumps(attempt['summary']))
                    if version == protocol.VERSION:
                        self.assertEqual(quiet_count, 5 if label == 'a' else 4)
                    result = TimingV4Tests().verdict(root, plan)
                    self.assertFalse(result['valid'])
                    for run in result['attempts']:
                        if run['pair'] in original_incomplete:
                            self.assertFalse(run['valid'])
                    self.assertTrue(all(s is not None for s in result['lost_sensitivity'].values()))


if __name__ == '__main__':
    unittest.main()


class FixedMeasurementThresholds(unittest.TestCase):
    """The amendment David accepted on 2026-09-13: the sustained background allowance and the idle
    floor are fixed like the burst ceiling, because calibration-derived ones got stricter the
    quieter the host was. Replays the real window-5 attempts that the old thresholds rejected."""

    WINDOW = Path('/Volumes/DS Vault/hub/scratch/fast3d/tmem-checks/T1/windows/window5-v4-validation'
                  '/native-demo1-dense/quiet')

    def replay(self, name, version):
        directory = self.WINDOW/name
        record = json.loads((directory/'run.json').read_text())
        samples = [json.loads(line) for line in (directory/'telemetry.jsonl').read_text().splitlines() if line.strip()]
        profile = {**record['resting_profile'], 'reasons': record['resting_profile'].get('reasons', [])}
        reasons = []
        for phase in ['preflight', 'run', 'postflight']:
            phase_samples = [s for s in samples if s.get('phase') == phase]
            # Contiguous history, as the runner keeps it: everything recorded before this phase.
            first = samples.index(phase_samples[0]) if phase_samples else 0
            history = samples[:first]
            if phase_samples:
                reasons += protocol.sample_reasons(phase_samples, phase, profile=profile,
                                                   allowed_idle=record.get('allowed_idle_services', ()),
                                                   history=history, version=version)
        return reasons

    def test_window5_rejections_were_calibration_artefacts(self):
        if not self.WINDOW.exists():
            self.skipTest('window5 evidence not present')
        for name in ['demo1-dense-coarse-3-candidate', 'demo1-dense-coarse-3-parent',
                     'demo1-dense-coarse-4-candidate']:
            with self.subTest(name=name):
                self.assertEqual(self.replay(name, protocol.V4), [],
                                 'fixed thresholds must admit the invocations the derived ones rejected')

    def test_fixed_thresholds_still_reject_real_noise(self):
        profile = {'thresholds': {'maximum_background_cores_average': .05,
                                  'maximum_background_cores_two_samples': .05,
                                  'minimum_idle_percent': 99.0}, 'reasons': []}
        loud = [{'phase': 'run', 'monotonic': float(i), 'background_cores': .9, 'idle_percent': 80.0,
                 'processes': [], 'disallowed': [], 'swapouts': 0, 'thermal_throttled': False,
                 'power': 'AC', 'power_mode': 'normal', 'idle_age_seconds': 0,
                 'monitor_elapsed_ms': 1, 'raw': ''} for i in range(5)]
        reasons = protocol.sample_reasons(loud, 'run', profile=profile, version=protocol.V4)
        self.assertTrue(any('ceiling' in r or 'allowance' in r or 'idle' in r for r in reasons), reasons)
