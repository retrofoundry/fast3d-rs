import copy
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import protocol
import report
from replay_batch import replay_directory
import t1
import test_timing_v4 as timing

CANDIDATE = '6392b66231ce5c1c8219aa3ee33a60f96652c9f1'
WINDOW = 'window7-t2-vs-t1'
FIXTURE = Path(__file__).with_name('fixtures')/'window6-verdict.json'


def change_metadata():
    return {'production_shas': {'parent':t1.PARENT, 'candidate':CANDIDATE},
            'builds': {label:{'binary_sha256':label} for label in ['parent','candidate']},
            'validation_verdict': {'path':str(FIXTURE), 'source_path':report.VALIDATION_PATH,
                                   'sha256':report.VALIDATION_SHA256}}


class CodeChangeTests(unittest.TestCase):
    def test_plan_selects_each_payload_and_keeps_pair_order(self):
        root, out = Path('/bundle'), Path('/attempt')
        plan = t1.native_plan(root, out, code_change=change_metadata())
        validation = t1.native_plan(root, out)
        self.assertIs(plan['same_binary_for_both_labels'], False)
        self.assertEqual(plan['production_shas'], change_metadata()['production_shas'])
        for entry, control in zip(plan['attempts'], validation['attempts']):
            self.assertEqual(entry['command'][0], str(root/'bin'/('b1-perf-'+entry['revision'])))
            entry['command'][0] = control['command'][0]
            self.assertEqual(entry, control)
        self.assertTrue(t1.metric_verdict([100]*5, [99]*5, .01, protocol.V4, identical=False)['resolved_speedup'])
        self.assertFalse(t1.metric_verdict([100]*5, [99]*5, .01, protocol.V4)['valid'])

    def test_native_audit_outcomes_and_missing_attachment_refusal(self):
        for delta, expected in [(1, 'regression'), (-1, 'resolved speedup'),
                                (.005, 'no regression resolved above U'), (None, 'inconclusive')]:
            with self.subTest(delta=delta), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                def change(values, window, entry, i):
                    if entry['revision'] == 'candidate':
                        values['cpu_ms'] += delta or 0
                plan = timing.TimingV4Tests().batch(root, change)
                plan.update(change_metadata(), same_binary_for_both_labels=False, settle_seconds=90,
                            warmup={'full_prefix_runs':1})
                frames = ''.join(json.dumps({'serial':i, 'observed':i >= 1400})+'\n' for i in range(1,1520))
                (root/'warmup').mkdir()
                (root/'warmup/frames.jsonl').write_text(frames)
                (root/'settle.json').write_text('{"elapsed_seconds":90}')
                for entry in plan['attempts']:
                    quiet = Path(entry['quiet_run'])
                    telemetry = quiet.with_name('telemetry.jsonl')
                    telemetry.write_text('{"processes":[]}\n')
                    run = json.loads(quiet.read_text())
                    run['telemetry'] = [{'path':str(telemetry),'sha256':protocol.sha(telemetry)}]
                    quiet.write_text(json.dumps(run))
                    target = Path(entry['summary']).parent
                    (target/'frames.jsonl').write_text(frames)
                    (target/'setup.json').write_text('{"readback":false,"frames_in_flight":2}')
                if delta is None:
                    Path(plan['attempts'][0]['quiet_run']).unlink()
                (root/'attempts.json').write_text(json.dumps(plan))
                result = t1.audit(root)
                self.assertEqual(result['status'], expected)
                self.assertEqual(result['valid'], delta in [-1, .005])
                self.assertEqual(result['production_shas'], change_metadata()['production_shas'])
                self.assertEqual(result['validation_verdict']['sha256'], report.VALIDATION_SHA256)
                self.assertTrue(result['lost_sensitivity'])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            plan = t1.native_plan(root, root, code_change=change_metadata())
            plan.pop('validation_verdict')
            (root/'attempts.json').write_text(json.dumps(plan))
            with self.assertRaisesRegex(ValueError, 'window6'):
                t1.audit(root)
            self.assertFalse((root/'verdict.json').exists())
            self.assertFalse((root/'batch.json').exists())

    def test_replay_keeps_direction_and_requires_the_relocated_attachment(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def change(values, window, entry, i):
                if entry['revision'] == 'candidate':
                    values['cpu_ms'] -= 1
            plan = timing.TimingV4Tests().batch(root, change)
            plan.update(change_metadata(), same_binary_for_both_labels=False)
            plan['validation_verdict']['path'] = str(root/'missing-worker-copy.json')
            (root/'attempts.json').write_text(json.dumps(plan))
            for entry in plan['attempts']:
                Path(entry['quiet_run']).with_name('telemetry.jsonl').write_text('')
            with self.assertRaisesRegex(ValueError, 'window6'):
                replay_directory(root, root/'refused', protocol.V4, 'native')
            self.assertFalse((root/'refused').exists())
            replay_directory(root, root/'replay', protocol.V4, 'native', FIXTURE)
            result = json.loads((root/'replay/replay.json').read_text())
            self.assertFalse(result['timing_valid'])
            self.assertEqual(result['metrics']['observed.cpu_ms']['conclusion'], 'resolved speedup')
            self.assertFalse(result['metrics']['observed.cpu_ms']['spurious_directional_change'])

    def authorization(self, root):
        proof = root/'acceptance.md'
        proof.write_text('David: One window now, T2 vs T1')
        entry = {'name':WINDOW, 'purpose':t1.CODE_CHANGE, 'accepted_by':'David',
                 'quote':'One window now, T2 vs T1', 'production_shas':change_metadata()['production_shas'],
                 'acceptance':{'path':str(proof),'sha256':protocol.sha(proof)}}
        return {'policy':protocol.V4, 'accepted_by':'David', 'windows':[entry],
                'acceptance':entry['acceptance'], 'prior_windows':[
                    {'path':'/external/window-a'}, {'path':str(root/'window1')}]}

    def test_reservation_requires_named_hashed_acceptance_and_consumes_once(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'window1').mkdir()
            auth = self.authorization(root)
            path = root/'validation-authorization.json'
            args = ('2026-09-13T00:00:00Z','2026-09-13T01:00:00Z','ci8','operator attestation')
            for defect in ['name','sha','quote','proof']:
                broken = copy.deepcopy(auth)
                if defect == 'name': broken['windows'][0]['name'] = 'unnamed'
                if defect == 'sha': broken['windows'][0]['production_shas']['candidate'] = '6392b66'
                if defect == 'quote': broken['windows'][0]['quote'] = ''
                if defect == 'proof': broken['windows'][0]['acceptance']['sha256'] = '0'*64
                path.write_text(json.dumps(broken))
                with self.assertRaises(ValueError):
                    t1.reserve(root, WINDOW, *args, version=protocol.V4)
                self.assertFalse((root/WINDOW).exists())
            path.write_text(json.dumps(auth))
            window = t1.reserve(root, WINDOW, *args, version=protocol.V4)
            reservation = json.loads((window/'reservation.json').read_text())
            self.assertEqual(reservation['purpose'], t1.CODE_CHANGE)
            self.assertEqual(reservation['production_shas'], change_metadata()['production_shas'])
            with self.assertRaises(ValueError):
                t1.reserve(root, WINDOW, *args, version=protocol.V4)

    def test_native_controls_both_binaries_before_one_warmup_and_settle(self):
        for mismatch in [False, True]:
            with self.subTest(mismatch=mismatch), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                window = root/WINDOW
                window.mkdir()
                (root/'kit.json').write_text('{}')
                auth = self.authorization(root)
                reservation = {'policy':protocol.V4, 'purpose':t1.CODE_CHANGE, 'authorization':auth,
                    'production_shas':change_metadata()['production_shas'],
                    'scheduled_start':'2026-09-13T00:00:00Z', 'scheduled_end':'2099-01-01T00:00:00Z',
                    'operator_attestation':'test reservation'}
                (window/'reservation.json').write_text(json.dumps(reservation))
                manifest = dict(change_metadata(), same_binary_for_both_labels=False)
                if mismatch: manifest['production_shas']['candidate'] = 'a'*40
                events = []
                with patch.object(t1, 'verify_kit', return_value=manifest), \
                        patch.object(t1, 'worker_probe', return_value={'errors':[]}), \
                        patch.object(t1.subprocess, 'run', side_effect=lambda argv, **kw:events.append(argv)), \
                        patch.object(t1.time, 'sleep', side_effect=lambda seconds:events.append(seconds)), \
                        patch.object(t1, 'audit', return_value={'valid':True}):
                    args = SimpleNamespace(root=root, window=window, cpu_floor_ms=.01, elapsed_floor_ms=.01)
                    if mismatch:
                        with self.assertRaisesRegex(ValueError, 'production SHAs'): t1.native(args)
                        self.assertFalse(events)
                        continue
                    self.assertEqual(t1.native(args), 0)
                for i, label in enumerate(['parent','candidate']):
                    self.assertIn(str(root/'bin'/('b1-perf-'+label)), events[i])
                self.assertEqual(events[2][0], str(root/'bin/b1-perf-parent'))
                self.assertEqual(events[3], 90)
                self.assertEqual(len(events[4:]), 10)
                self.assertEqual([argv[-5] for argv in events[4:]],
                    [a['command'][0] for a in t1.native_plan(root, window/'native-demo1-dense', code_change=manifest)['attempts']])

    def test_freeze_and_relay_verify_each_build_and_the_attachment(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['bin','provenance/parent','provenance/candidate','validation','source','python/bin','scripts']:
                (root/name).mkdir(parents=True, exist_ok=True)
            for label, revision in change_metadata()['production_shas'].items():
                binary = root/'bin'/('b1-perf-'+label)
                binary.write_text(label)
                provenance = root/'provenance'/label
                build = {'production_sha':revision, 'binary_sha256':protocol.sha(binary), 'rustc':'test rustc',
                         'assembler_sha':'a'*40, 'features':['capture','profiling'], 'python_distribution':{'release':'test'}}
                for name, field, data in [('driver-source.tar.gz','driver_source_sha256','source'),
                        ('dependencies.json','dependency_graph_sha256','{"packages":[1],"resolve":{"nodes":[1]}}'),
                        ('Cargo.lock','lock_sha256','lock')]:
                    path = provenance/name
                    path.write_text(data)
                    build[field] = protocol.sha(path)
                (provenance/'build.json').write_text(json.dumps(build))
            (root/'validation/window6-verdict.json').write_bytes(FIXTURE.read_bytes())
            for name in ['source/chrome-icosphere.n64','python/bin/python3', *['scripts/'+s for s in t1.SCRIPTS]]:
                (root/name).write_text(name)
            (root/'source/input.json').write_text(json.dumps({'source_sha256':protocol.sha(root/'source/chrome-icosphere.n64'),
                'synthetic':True, 'generator':'env-xor-v1', 'frames':[0,719], 'warmup':[0,119], 'observed':[120,719], 'seed':0}))
            with patch.object(t1, 'CAPTURES', {}):
                t1.freeze(SimpleNamespace(root=root, parent_sha=t1.PARENT, candidate_sha=CANDIDATE))
            manifest = t1.verify_kit(root)
            self.assertIs(manifest['same_binary_for_both_labels'], False)
            self.assertTrue(all(row['command_argv'][0] == '{bundle}/bin/b1-perf-{revision}'
                                for row in manifest['rows'] if row['backend'] == 'native'))
            for label in ['parent','candidate']:
                self.assertEqual(manifest['builds'][label]['binary_sha256'], protocol.sha(root/'bin'/('b1-perf-'+label)))
            for target in ['bin/b1-perf-parent','bin/b1-perf-candidate','provenance/candidate/Cargo.lock',
                           'provenance/parent/build.json','validation/window6-verdict.json']:
                path = root/target
                original = path.read_bytes()
                path.write_bytes(original+b'changed')
                with self.assertRaises(ValueError): t1.verify_kit(root)
                path.write_bytes(original)
