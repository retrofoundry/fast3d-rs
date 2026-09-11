import copy
import gzip
import json
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path
from types import SimpleNamespace

import protocol


class QuietTests(unittest.TestCase):
    def recorded_samples(self):
        with gzip.open(Path(__file__).with_name('fixtures')/'ci4-dryrun.jsonl.gz', 'rt') as stream:
            return [json.loads(line) for line in stream]

    def test_recorded_idle_host_passes_retrospective_check(self):
        result = protocol.replay(self.recorded_samples())
        self.assertTrue(result['quiet_valid'], result['reasons'])
        self.assertFalse(result['timing_valid'])
        self.assertEqual(result['usable_intervals'], 29)
        names = [p['command'] for p in result['resting_profile']['idle_processes']]
        self.assertTrue(any('/claude/versions/' in name for name in names))
        self.assertTrue(any('Google Chrome' in name for name in names))

    def test_build_cannot_be_calibrated_away_or_exempted(self):
        for exemption in (None, 'benchmark'):
            samples = self.recorded_samples()
            for row in samples:
                row['processes'].append({'pid':999999, 'ppid':1, 'command':'/toolchain/bin/cargo',
                                         'cpu_seconds':0, 'cores':0, 'exemption':exemption})
            result = protocol.replay(samples, allowed_idle=['cargo'])
            self.assertFalse(result['quiet_valid'])
            self.assertTrue(any('/toolchain/bin/cargo' in reason for reason in result['reasons']))

    def test_initial_cpu_is_baseline_and_monitor_is_excluded_by_ownership(self):
        samples = self.recorded_samples()
        corrected = protocol.recalculate(samples)
        self.assertEqual(len(corrected), 29)
        first = corrected[0]
        self.assertEqual(next(p['cores'] for p in first['processes'] if p['pid']==1), 0)
        expected = sum(p['cores'] or 0 for p in first['processes'] if not p['exemption'])
        self.assertAlmostEqual(first['background_cores'], expected)
        changed = copy.deepcopy(samples)
        for i, row in enumerate(changed):
            for p in row['processes']:
                if p['exemption']=='monitor':
                    p['cpu_seconds'] += i*100
        self.assertEqual([s['background_cores'] for s in corrected],
                         [s['background_cores'] for s in protocol.recalculate(changed)])

    def test_new_baseline_does_not_charge_lifetime_cpu(self):
        processes = {1:{'pid':1,'ppid':0,'command':'/sbin/launchd','cpu_seconds':6000}}
        protocol.account_processes(processes, None, 1, set(), set())
        self.assertIsNone(processes[1]['cores'])
        next_processes = copy.deepcopy(processes)
        next_processes[1]['cpu_seconds'] += .01
        protocol.account_processes(next_processes, processes, 1, set(), set())
        self.assertAlmostEqual(next_processes[1]['cores'], .01)

    def test_busy_agent_is_named_and_not_absorbed_into_reference(self):
        samples = self.recorded_samples()
        for i, row in enumerate(samples):
            for p in row['processes']:
                if p['pid']==1677:
                    p['cpu_seconds'] = 100+i
        result = protocol.replay(samples)
        self.assertFalse(result['quiet_valid'])
        self.assertTrue(any('/claude/versions/' in reason for reason in result['reasons']))

    def test_build_in_baseline_is_still_named(self):
        samples = self.recorded_samples()
        samples[0]['processes'].append({'pid':999999, 'ppid':1, 'command':'cargo',
                                       'cpu_seconds':100, 'cores':None, 'exemption':None})
        result = protocol.replay(samples)
        self.assertFalse(result['quiet_valid'])
        self.assertTrue(any('cargo' in reason for reason in result['reasons']))

    def test_new_short_lived_process_is_charged_after_initial_baseline(self):
        processes = {99:{'pid':99,'ppid':1,'command':'claude','cpu_seconds':.9}}
        protocol.account_processes(processes, {}, 1, set(), set())
        self.assertEqual(processes[99]['cores'], .9)

    def test_monitor_exempts_owned_probes_but_not_reused_benchmark_pid(self):
        source = self.recorded_samples()[0]
        monitor = protocol.Monitor.__new__(protocol.Monitor)
        monitor.benchmark_root = None
        monitor.benchmark_process = None
        monitor.benchmark_pids = {456:'Fri Sep 11 10:00:00 2026'}
        monitor.last = (0, {})
        monitor.idle = (1, 95)
        raw = dict(source['raw'])
        raw['processes'] = '\n'.join([
            '123 1 Fri Sep 11 10:00:00 2026 0:01.00 Python',
            '124 123 Fri Sep 11 10:00:00 2026 0:01.00 top',
            '125 123 Fri Sep 11 10:00:00 2026 0:01.00 ps',
            '126 1 Fri Sep 11 10:00:00 2026 0:01.00 top',
            '456 1 Fri Sep 11 10:00:01 2026 0:02.00 /opt/tools/claude'])
        def recorded_command(args):
            return next(raw[key] for key, value in protocol.COMMANDS.items() if args==value)
        with patch('protocol.command', side_effect=recorded_command), patch('protocol.os.getpid', return_value=123), patch('protocol.time.monotonic', return_value=1):
            sample = monitor.take('run')
        self.assertEqual(sample['background_cores'], 3)
        replacement = next(p for p in sample['processes'] if p['pid']==456)
        self.assertIsNone(replacement['exemption'])
        self.assertTrue(any('/opt/tools/claude' in r for r in protocol.process_activity([sample], 'run')[1]))

    def test_reference_is_required_and_added_load_fails(self):
        samples = protocol.recalculate(self.recorded_samples())
        profile = protocol.resting_profile(samples)
        self.assertIn('missing resting profile', protocol.sample_reasons(samples, 'run'))
        for sample in samples:
            sample['background_cores'] += 1
        reasons = protocol.sample_reasons(samples, 'run', profile)
        self.assertTrue(any('background' in reason for reason in reasons))

    def test_short_run_uses_preflight_history_for_activity_average(self):
        samples = protocol.recalculate(self.recorded_samples())
        profile = protocol.resting_profile(samples)
        repeated = [copy.deepcopy(s) for s in samples*3]
        for i, sample in enumerate(repeated):
            sample['monotonic'] = float(i)
        for offset in range(29, 53):
            history = repeated[offset-29:offset]
            run = repeated[offset:offset+5]
            self.assertEqual(protocol.sample_reasons(run, 'run', profile, history=history), [])

    def test_phase_transition_is_not_a_cadence_gap_but_long_blind_spots_fail(self):
        samples = protocol.recalculate(self.recorded_samples())
        profile = protocol.resting_profile(samples)
        history = copy.deepcopy(samples)
        for i, row in enumerate(history):
            row.update(phase='preflight', monotonic=float(i))
        run = copy.deepcopy(samples[:4])
        for i, row in enumerate(run):
            row.update(phase='run', monotonic=len(history)-1+1.56+i)
        self.assertNotIn('monitor gap', protocol.sample_reasons(run,'run',profile,history=history))
        run[2]['monotonic'] += .7
        self.assertIn('monitor gap', protocol.sample_reasons(run,'run',profile,history=history))
        for i, row in enumerate(run):
            row['monotonic'] = len(history)+4+i
        self.assertIn('phase transition gap', protocol.sample_reasons(run,'run',profile,history=history))

    def test_idle_compares_trimmed_means_and_ignores_one_noise_sample(self):
        samples = protocol.recalculate(self.recorded_samples())
        for row in samples:
            row['idle_percent'] = 94
        profile = protocol.resting_profile(samples)
        for count in [10,29]:
            selected = copy.deepcopy(samples[:count])
            selected[0]['idle_percent'] = 89.9
            reasons = protocol.sample_reasons(selected,'replay',profile)
            self.assertFalse(any('idle trimmed mean' in r for r in reasons), reasons)
            for row in selected:
                row['idle_percent'] = 90
            self.assertTrue(any('idle trimmed mean' in r for r in protocol.sample_reasons(selected,'replay',profile)))

    def test_aggregate_dirty_calibration_cannot_loosen_thresholds(self):
        samples = protocol.recalculate(self.recorded_samples())
        for row in samples:
            row.update(background_cores=.56, idle_percent=76)
        profile = protocol.resting_profile(samples)
        self.assertTrue(any('dirty calibration: background average' in r for r in profile['reasons']))
        self.assertTrue(any('dirty calibration: idle p20' in r for r in profile['reasons']))
        self.assertGreaterEqual(profile['thresholds']['minimum_idle_percent'],90)
        self.assertTrue(protocol.sample_reasons(samples,'run',profile))

    def test_phase_duration_allows_endpoint_probe_jitter(self):
        samples = protocol.recalculate(self.recorded_samples())
        samples.append(copy.deepcopy(samples[-1]))
        for i, row in enumerate(samples):
            row['monotonic'] = float(i)
        samples[-1]['monotonic'] -= .02
        self.assertNotIn('incomplete 30-second resting', protocol.sample_reasons(samples,'resting'))
        for i, row in enumerate(samples):
            row['monotonic'] = i*.5
        self.assertIn('incomplete 30-second resting', protocol.sample_reasons(samples,'resting'))

    def test_safe_browsing_is_a_background_service_not_a_gpu_producer(self):
        name = '/System/Library/PrivateFrameworks/SafariSafeBrowsing.framework/com.apple.Safari.SafeBrowsing.Service'
        self.assertFalse(protocol.idle_process(name, []))
        self.assertTrue(protocol.idle_process('/Applications/Safari.app/Contents/MacOS/Safari', []))

    def test_attempt_archives_profile_and_runs_sleep_only_after_quiet_admission(self):
        recorded = protocol.recalculate(self.recorded_samples())
        class RecordedMonitor:
            def __init__(self, allowed_idle):
                self.index = 0
                self.top_raw = []
            def take(self, phase):
                sample = copy.deepcopy(recorded[self.index % len(recorded)])
                sample.update(monotonic=float(self.index), phase=phase)
                self.index += 1
                if build and phase=='resting':
                    sample['processes'].append({'pid':999999,'ppid':1,'command':'cargo',
                                                'cores':0,'cpu_seconds':0,'exemption':None})
                return sample
            def close(self):
                pass
        with tempfile.TemporaryDirectory() as directory:
            for build in (False, True):
                out = Path(directory)/str(build)
                args = SimpleNamespace(out=out, scheduled_start='2000-01-01T00:00:00Z',
                                       scheduled_end='2100-01-01T00:00:00Z', attestation='test only',
                                       allowed_idle=[], command=['sleep','.05'], pair='test',
                                       revision='candidate', workload='sleep', configuration='counters')
                with patch('protocol.Monitor', RecordedMonitor), patch('protocol.time.sleep'):
                    code = protocol.quiet(args)
                result = json.loads((out/'run.json').read_text())
                self.assertEqual(result['valid'], not build, result['reasons'])
                self.assertEqual(code, int(build))
                self.assertEqual(result['exit_code'], None if build else 0)
                self.assertEqual(result['resting_profile']['sha256'], protocol.sha(out/'resting-profile.json'))
                self.assertTrue(any('cargo' in r for r in result['reasons']) if build else not result['reasons'])


if __name__ == '__main__':
    unittest.main()
