#!/usr/bin/env python3
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys
import time
from types import SimpleNamespace

from protocol import POLICY, VERSION, V4, V4_POLICY, command, sha, timing_policy, utc
from report import CAPTURES, batch_report, batch_metrics, compare, metric_verdict as compare_metric, rows, summarize

PARENT = 'bb0fea399793eb2aeb1ae93d5972763b9761da98'
SCRIPTS = ['t1.py', 'protocol.py', 'report.py', 'replay_batch.py']
METRICS = ['cpu_ms', 'emission_interval_ms']
NATIVE_PILOT = 'native.demo1-dense'
V4_WINDOW = 'window3-v4-validation'


def v4_windows(authorization):
    # The authorization file lists every window David has accepted, in order. It stays data:
    # a retry after a diagnosed failure is his decision, recorded with his words, not a code edit.
    names = authorization.get('windows') or ([authorization['window']] if authorization.get('window') else [])
    return [entry['name'] if isinstance(entry, dict) else entry for entry in names]


def write(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2, allow_nan=False)


def read(path):
    return json.loads(path.read_text())


def row_manifest():
    result = []
    for backend in ['native', 'chrome']:
        for workload in ['demo1-dense', 'demo2', 'source', 'demo1', 'pinned']:
            capture = CAPTURES.get(workload)
            if backend == 'native':
                argv = ['{bundle}/bin/b1-perf', 'sequence' if capture else 'source',
                        '{bundle}/captures/'+workload+'.f3dcap' if capture else '{bundle}/source/chrome-icosphere.n64',
                        '{fresh-output}', '{configuration}']
            else:
                argv = ['{python}', '{repo}/tools/perf/browser_run.py', '--build', '{bb0fea3-browser-build}',
                        '--scene', '{scene}', '--out', '{fresh-output}', '--chromedriver', '{chromedriver}',
                        '--webdriver', '{repo}/fast3d/webdriver.json', '--mode', '{configuration}',
                        '--kind', 'sequence' if capture else 'source']
                if capture:
                    argv += ['--capture', '{captures}/'+workload+'.f3dcap']
            result.append({
                'id': f'{backend}.{workload}', 'backend': backend, 'workload': workload,
                'role': 'control' if workload in ['demo1', 'pinned'] else 'primary',
                'prefix': [1, capture['frames']] if capture else [0, 719],
                'warmup': capture['warmup'] if capture else 120,
                'observed': capture['observed'] if capture else [120, 719],
                'input_sha256': capture['sha256'] if capture else 'source/input.json',
                'seed': 'recorded capture seeds' if capture else 0,
                'presentations': 'all retained frames; preserve capture declarations',
                'configuration': ['coarse', 'counters'], 'pairs': 5,
                'policies': {'coarse': V4 if (backend, workload) == ('native', 'demo1-dense') else VERSION,
                             'counters': VERSION},
                'command_argv': argv,
                'driver_control_options': [[], ['--readback'], ['--readback', '--one-frame']],
                'frames_in_flight': 2, 'timed_readback': False,
                'cpu_boundary': 'begin_frame + process_dl + presentation; excludes assembly, adaptation, waits, readback, serialization',
                'elapsed_metric': 'observed emission_interval_ms',
                'cold_metrics': ['device_ms', 'renderer/pipeline setup', 'first frame CPU', 'cold prefix CPU and elapsed'],
                'cache_provider': 'bb0fea3 production CPU path; no new memo/residency/provider; diagnostic LRU budget 16777216 bytes only in separate trace passes',
                'driver_admission': 'blocked-until-executed',
                'requirements': 'full prefix, original feature declarations and memory; exact readback control, one-frame control; Chrome teardown complete and zero survivors',
            })
    return result


def native_plan(root, out, version=V4):
    attempts = []
    for pair in range(1, 6):
        for revision in (['parent', 'candidate'] if pair % 2 else ['candidate', 'parent']):
            name = f'demo1-dense-coarse-{pair}-{revision}'
            timed = out/'timed'/name
            attempts.append({
                'pair': f'demo1-dense-coarse-{pair}', 'revision': revision,
                'backend': 'native', 'workload': 'demo1-dense', 'configuration': 'coarse',
                'quiet_run': str(out/'quiet'/name/'run.json'),
                'summary': str(timed/'summary.json'), 'expected_observed': 120,
                'command': [str(root/'bin/b1-perf'), 'sequence',
                            str(root/'captures/demo1-dense.f3dcap'), str(timed), 'coarse'],
            })
    return {'production_sha': PARENT, 'same_binary_for_both_labels': True,
            'policy': version, 'backend': 'native', 'workload': 'demo1-dense', 'configuration': 'coarse',
            'cells': ['parent.demo1-dense.coarse', 'candidate.demo1-dense.coarse'],
            'attempts': attempts}


def metric_verdict(parent, candidate, floor, version=VERSION, limit=.05):
    return compare_metric(parent, candidate, floor, version, limit, identical=True)


def frame_errors(frames):
    errors = []
    if [f.get('serial') for f in frames] != list(range(1, 1520)):
        errors.append('missing or changed full prefix 1..1519')
    if [f.get('serial') for f in frames if f.get('observed') is True] != list(range(1400, 1520)):
        errors.append('missing or changed observed window 1400..1519')
    if any(f.get('rgba8_sha256') is not None or f.get('readback_ms', 0) != 0 for f in frames):
        errors.append('readback in timed run')
    return errors


def reserve(root, name, start, end, host, attestation, version=VERSION):
    if not re.fullmatch(r'[a-zA-Z0-9_-]+', name) or not host or not attestation.strip():
        raise ValueError('window ID, named host and operator attestation required')
    if host.lower() == 'ci4':
        raise ValueError('native timing reservations require a blaze worker; ci4 Chrome driver admission is separate')
    interval = [datetime.fromisoformat(v.replace('Z', '+00:00')) for v in [start, end]]
    if any(v.tzinfo is None for v in interval) or interval[0] >= interval[1]:
        raise ValueError('reservation must have an increasing timezone-aware UTC interval')
    root.mkdir(parents=True, exist_ok=True)
    windows = [p for p in root.iterdir() if p.is_dir()]
    authorization_path = root/'validation-authorization.json'
    authorization = read(authorization_path) if authorization_path.exists() else {}
    prior = authorization.get('prior_windows', [])
    external = sum(not Path(p['path']).resolve().is_relative_to(root.resolve()) for p in prior)
    if version == V4:
        accepted = v4_windows(authorization)
        consumed = [p.name for p in windows if (p/'reservation.json').exists()
                    and read(p/'reservation.json').get('policy') == V4]
        if (name not in accepted or authorization.get('policy') != V4 or
                len(prior) != 2 + len(consumed) or
                len(windows) + external != 2 + len(consumed) or
                authorization.get('accepted_by') != 'David' or
                sha(Path(authorization['acceptance']['path'])) != authorization['acceptance']['sha256']):
            raise ValueError(f'requires a recorded David acceptance naming {name}')
        if name in consumed:
            raise ValueError(f'{name} is already consumed')
        if accepted.index(name) != len(consumed):
            raise ValueError('accepted v4 windows are consumed in the order David recorded them')
    elif version != VERSION or name in v4_windows(authorization) or len(windows) + external >= 2:
        raise ValueError('two reserved windows exhausted; stop for the T1 protocol decision')
    window = root/name
    window.mkdir()
    write(window/'reservation.json', {'host': host, 'scheduled_start': start,
          'scheduled_end': end, 'operator_attestation': attestation,
          'production_sha': PARENT, 'policy': version, 'created_utc': utc(),
          'purpose': 'accepted-v4-parent-parent-validation' if version == V4 else 'v3-pilot',
          'authorization': authorization if version == V4 else None})
    return window


def recorded_sensitivity(attempt, version):
    runs, summaries = [], {}
    for path in sorted((attempt/'timed').glob('*/summary.json')):
        record = attempt/'quiet'/path.parent.name/'run.json'
        if record.exists():
            summaries[str(path)] = read(path)
            runs.append({**read(record), 'artifacts':{'summary':str(path)}})
    return {key:value['lost_sensitivity'] for key,value in
            batch_metrics({}, runs, summaries, version).items()}


def decision(root):
    windows = sorted(p for p in root.iterdir() if p.is_dir()) if root.exists() else []
    auth_path = root/'validation-authorization.json'
    authorization = read(auth_path) if auth_path.exists() else {}
    version = V4 if authorization.get('policy') == V4 else VERSION
    evidence = []
    sensitivity = {}
    for window in windows:
        reservation = read(window/'reservation.json') if (window/'reservation.json').exists() else {}
        for path in sorted(window.glob('*/verdict.json')):
            result = read(path)
            sensitivity[window.name] = result.get('lost_sensitivity') or recorded_sensitivity(path.parent, result['policy'])
            evidence.append({'path': str(path), 'sha256': sha(path), **result,
                             'reservation_policy': reservation.get('policy'), 'window': window.name})
    passed = [r for r in evidence if r.get('valid') and r.get('policy') == version and
              r.get('reservation_policy') == version and r.get('cell') == NATIVE_PILOT and
              (version != V4 or r['window'] == V4_WINDOW)]
    external = sum(not Path(p['path']).resolve().is_relative_to(root.resolve()) for p in authorization.get('prior_windows', []))
    consumed = len(windows) + external
    for prior in authorization.get('prior_windows', []):
        path = Path(prior['path'])
        if not path.resolve().is_relative_to(root.resolve()):
            sensitivity[prior['name']] = recorded_sensitivity(path, prior['policy'])
    valid = bool(passed) and consumed <= (3 if version == V4 else 2)
    if version == V4:
        proof = authorization.get('acceptance', {})
        valid = valid and (authorization.get('window') == V4_WINDOW and
                authorization.get('accepted_by') == 'David' and
                Path(proof.get('path', '')).is_file() and
                sha(Path(proof['path'])) == proof.get('sha256'))
    pending_v4 = version == V4 and not (root/V4_WINDOW).exists()
    return {'valid': valid, 'policy': version,
            'lost_sensitivity': sensitivity,
            'status': 'native-pilot-pass' if valid else 'awaiting-v4-validation' if pending_v4 else
            'stop-protocol-decision' if consumed >= (3 if version == V4 else 2) else 'inconclusive',
            'reserved_windows': consumed, 'missing_cells': [] if valid else [NATIVE_PILOT],
            'prior_windows': authorization.get('prior_windows', []), 'evidence': evidence,
            'chrome_driver_admission': {'required': True, 'host': 'ci4',
                                        'status': 'pending-separate-cell', 'timing_valid': False},
            'full_t1_valid': False,
            'scope': 'native worker dry-run feasibility only; Chrome driver admission on ci4 and the full ten-row timing/count/cold matrix remain separate gates'}


def freeze(args):
    root = args.root.resolve()
    build = read(root/'provenance/build.json')
    if build.get('production_sha') != PARENT or build.get('binary_sha256') != sha(root/'bin/b1-perf'):
        raise ValueError('build record must identify the exact bb0fea3 binary')
    for field in ['driver_source_sha256', 'dependency_graph_sha256', 'lock_sha256',
                  'rustc', 'assembler_sha', 'features', 'python_distribution']:
        if not build.get(field):
            raise ValueError(f'missing build provenance: {field}')
    for name, field in [('driver-source.tar.gz', 'driver_source_sha256'),
                        ('dependencies.json', 'dependency_graph_sha256'), ('Cargo.lock', 'lock_sha256')]:
        if sha(root/'provenance'/name) != build[field]:
            raise ValueError(f'build provenance hash mismatch: {name}')
    graph = read(root/'provenance/dependencies.json')
    if not graph.get('packages') or not graph.get('resolve', {}).get('nodes'):
        raise ValueError('dependency graph must contain resolved packages')
    for name, expected in CAPTURES.items():
        if sha(root/'captures'/(name+'.f3dcap')) != expected['sha256']:
            raise ValueError(f'capture hash mismatch: {name}')
    required = ['bin/b1-perf', 'source/chrome-icosphere.n64', 'source/input.json',
                'python/bin/python3', *['scripts/'+name for name in SCRIPTS]]
    for name in required:
        if not (root/name).is_file():
            raise ValueError(f'missing relay file: {name}')
    source = read(root/'source/input.json')
    if source.get('source_sha256') != sha(root/'source/chrome-icosphere.n64'):
        raise ValueError('source metadata hash mismatch')
    expected_source = {'synthetic': True, 'generator': 'env-xor-v1', 'frames': [0, 719],
                       'warmup': [0, 119], 'observed': [120, 719], 'seed': 0}
    if any(source.get(key) != value for key, value in expected_source.items()):
        raise ValueError('source schedule or synthetic texture declaration changed')
    files = {str(p.relative_to(root)): sha(p) for p in sorted(root.rglob('*'))
             if p.is_file() and p.name != 'kit.json' and '__pycache__' not in p.parts}
    write(root/'kit.json', {'production_sha': PARENT, 'policy': V4, 'quiet_policy': V4_POLICY,
          'policies': {VERSION: POLICY, V4: V4_POLICY},
          'files': files, 'build': build, 'rows': row_manifest(), 'created_utc': utc(),
          'host_contract': 'drained blaze Apple M2, 8 logical cores, 24 GiB, macOS 26.x; orchard unloaded; no Chrome',
          'source': source})
    return 0


def verify_kit(root):
    manifest = read(root/'kit.json')
    if manifest['production_sha'] != PARENT or manifest['policy'] != V4 or manifest['quiet_policy'] != V4_POLICY or manifest.get('policies') != {VERSION: POLICY, V4: V4_POLICY}:
        raise ValueError('kit parent or quiet policy changed')
    for name, expected in manifest['files'].items():
        if sha(root/name) != expected:
            raise ValueError(f'relay hash mismatch: {name}')
    return manifest


def worker_probe(reservation):
    probes = {
        'hostname': ['scutil', '--get', 'LocalHostName'], 'cpu': ['sysctl', '-n', 'machdep.cpu.brand_string'],
        'cores': ['sysctl', '-n', 'hw.logicalcpu'], 'memory': ['sysctl', '-n', 'hw.memsize'],
        'os': ['sw_vers', '-productVersion'], 'jobs': ['launchctl', 'list'],
        'processes': ['ps', '-ww', '-axo', 'comm='], 'power': ['pmset', '-g', 'batt'],
        'power_mode': ['pmset', '-g', 'custom'],
    }
    result = {key: command(argv).strip() for key, argv in probes.items()}
    errors = []
    if result['hostname'].lower() != reservation['host'].lower() or result['hostname'].lower() == 'ci4':
        errors.append('requires the named drained blaze worker; ci4 is not native worker capacity')
    if (result['cpu'] != 'Apple M2' or result['cores'] != '8' or
            result['memory'] != '25769803776' or not result['os'].startswith('26.')):
        errors.append('requires Apple M2 / 8 cores / 24 GiB / macOS 26.x')
    if sys.version_info[:2] != (3, 12) or not Path(sys.executable).resolve().is_relative_to(Path.home()):
        errors.append('requires the relayed python-build-standalone 3.12 under the worker home')
    if re.search(r'dev\.runblaze\.worker', result['jobs']):
        errors.append('orchard worker is still loaded')
    if re.search(r'Google Chrome|Chromium|chrome_crashpad', result['processes'], re.I):
        errors.append('Chrome is present on the native worker')
    result['errors'] = errors
    return result


def driver_checks(binary, out):
    out.mkdir()
    for mode in ['coarse', 'counters']:
        target = out/mode
        subprocess.run([str(binary), 'sequence', 'authored', str(target), mode], check=True)
        summarize(SimpleNamespace(input=target/'frames.jsonl', out=target/'summary.json',
                                  configuration=mode, expected_observed=4))
        if not read(target/'summary.json')['validation']['valid']:
            raise ValueError(f'{mode} driver summary rejected')
    ordinary = out/'ordinary'
    subprocess.run([str(binary), 'ordinary', 'authored', str(ordinary)], check=True)
    for name, options in [('readback', ['--readback']), ('one-frame', ['--readback', '--one-frame'])]:
        target = out/name
        subprocess.run([str(binary), 'sequence', 'authored', str(target), 'coarse', *options], check=True)
        compare(SimpleNamespace(input=ordinary/'frames.jsonl', other=target/'frames.jsonl', out=target/'comparison.json'))
        if not read(target/'comparison.json')['valid']:
            raise ValueError(f'{name} differs from ordinary replay')


def browser_admit(workload, coarse, readback, one_frame, out):
    if any(path.exists() for path in [out, out.with_suffix('.summary.json'), out.with_suffix('.comparison.json')]):
        raise FileExistsError('driver admission outputs must be fresh')
    row = next(r for r in row_manifest() if r['id'] == 'chrome.'+workload)
    errors = []
    evidence = []
    for directory in [coarse, readback, one_frame]:
        try:
            result = read(directory/'result.json')
            teardown = read(directory/'browser-teardown.json')
            if not teardown['complete'] or teardown['survivors']:
                errors.append(f'{directory}: incomplete browser teardown')
            frames = result['frames']
            if [f.get('serial') for f in frames] != list(range(row['prefix'][0], row['prefix'][1]+1)):
                errors.append(f'{directory}: missing full prefix')
            if [f.get('serial') for f in frames if f.get('observed') is True] != list(range(row['observed'][0], row['observed'][1]+1)):
                errors.append(f'{directory}: missing observed window')
            if directory == coarse and any(f.get('rgba8_sha256') is not None or f.get('readback_ms', 0) != 0 for f in frames):
                errors.append(f'{directory}: readback in coarse driver check')
            metadata = result if workload != 'source' else result['setup']
            if metadata.get('frames_in_flight') != (1 if directory == one_frame else 2):
                errors.append(f'{directory}: wrong completion bound')
            for field in ['adapter', 'features', 'limits']:
                if not metadata.get(field):
                    errors.append(f'{directory}: missing {field}')
            for name in ['result.json', 'result.jsonl', 'browser-capabilities.json', 'browser-teardown.json']:
                evidence.append({'path': str(directory/name), 'sha256': sha(directory/name)})
        except (OSError, ValueError, KeyError, TypeError) as error:
            errors.append(str(error))
    if not errors:
        try:
            summarize(SimpleNamespace(input=coarse/'result.jsonl', out=out.with_suffix('.summary.json'),
                                      configuration='coarse', expected_observed=row['observed'][1]-row['observed'][0]+1))
            compare(SimpleNamespace(input=readback/'result.jsonl', other=one_frame/'result.jsonl',
                                    out=out.with_suffix('.comparison.json')))
        except (ValueError, SystemExit) as error:
            errors.append(f'coarse summary or readback control rejected: {error}')
    result = {'valid': not errors, 'timing_valid': False, 'cell': row['id'],
              'status': 'driver-admitted' if not errors else 'blocked',
              'reasons': errors, 'artifacts': evidence,
              'scope': 'untimed ci4 driver/prefix/readback/completion admission, separate from the native worker dry run; downstream Chrome timing acceptance remains required'}
    write(out, result)
    return result


def audit(out):
    if (out/'verdict.json').exists() or (out/'batch.json').exists():
        raise FileExistsError('completed attempt is immutable; use replay_batch.py with a fresh output directory')
    plan = read(out/'attempts.json')
    version = plan.get('policy', VERSION)
    timing_policy(version, plan.get('backend'), plan.get('workload'), plan.get('configuration'))
    reasons = []
    if version == V4:
        warmup = plan.get('warmup', {})
        if (plan.get('settle_seconds') != 90 or warmup.get('full_prefix_runs') != 1 or
                not (out/'warmup/frames.jsonl').exists()):
            reasons.append('missing one throwaway full-prefix warm-up before fixed 90-second settle')
        else:
            reasons.extend(frame_errors(list(rows(out/'warmup/frames.jsonl'))))
        if not (out/'settle.json').exists() or read(out/'settle.json').get('elapsed_seconds', 0) < 90:
            reasons.append('missing completed 90-second settle record')
    if any(not Path(a['quiet_run']).exists() for a in plan['attempts']):
        reasons.append('interrupted or unlaunched protocol invocation; retain the complete attempt')
    else:
        for entry in plan['attempts']:
            target = Path(entry['summary']).parent
            try:
                record = read(Path(entry['quiet_run']))
                if record.get('command') != entry['command']:
                    reasons.append('protocol command differs from the frozen identical-binary command')
                for evidence in record.get('telemetry', []):
                    if sha(Path(evidence['path'])) != evidence['sha256']:
                        reasons.append('telemetry hash changed')
                if not record.get('telemetry'):
                    reasons.append('missing telemetry')
                telemetry = Path(entry['quiet_run']).with_name('telemetry.jsonl')
                for sample in rows(telemetry):
                    if any(re.search(r'Google Chrome|Chromium|chrome_crashpad', p['command'], re.I)
                           for p in sample['processes']):
                        reasons.append('Chrome present during native calibration or reservation')
                if (target/'frames.jsonl').exists():
                    frames = list(rows(target/'frames.jsonl'))
                    reasons.extend(frame_errors(frames))
                    setup = read(target/'setup.json')
                    if setup.get('readback') is not False or setup.get('frames_in_flight') != 2:
                        reasons.append('timed setup requires no readback and two-frame completion bound')
                    if not (target/'summary.json').exists():
                        summarize(SimpleNamespace(input=target/'frames.jsonl', out=target/'summary.json',
                                                  configuration='coarse', expected_observed=120))
                else:
                    reasons.append('missing full-prefix frame records')
            except (OSError, ValueError, KeyError, TypeError) as error:
                reasons.append(f'{entry["pair"]}/{entry["revision"]}: {error}')
        try:
            batch_report(SimpleNamespace(input=out/'attempts.json', out=out/'batch.json'))
            if not read(out/'batch.json')['valid']:
                reasons.append('report.py rejected the paired batch')
        except (OSError, ValueError, KeyError, TypeError) as error:
            reasons.append(f'report.py admission incomplete: {error}')
    summaries = {a['summary']: read(Path(a['summary'])) for a in plan['attempts'] if Path(a['summary']).exists()}
    described_runs = [{**a, 'artifacts':a} for a in plan['attempts']]
    descriptive = batch_metrics(plan, described_runs, summaries, version)
    metrics = read(out/'batch.json').get('metrics', {}) if (out/'batch.json').exists() else {}
    if not reasons and version == VERSION:
        for metric in METRICS:
            totals = {revision: [read(Path(a['summary']))['observed'][metric] for a in plan['attempts']
                                 if a['revision'] == revision] for revision in ['parent', 'candidate']}
            metrics[metric] = metric_verdict(totals['parent'], totals['candidate'], plan['floors_ms'][metric])
            if not metrics[metric]['valid']:
                reasons.append(f'{metric}: identical-binary spread or directional-bias rejection')
    result = {'valid': not reasons, 'cell': 'native.demo1-dense', 'policy': version,
              'same_binary_for_both_labels': plan.get('same_binary_for_both_labels', False),
              'production_sha': PARENT, 'reasons': sorted(set(reasons)), 'metrics': metrics,
              'lost_sensitivity': {key:value['lost_sensitivity'] for key,value in descriptive.items()},
              'sensitivity_scope': 'All recorded timings, including rejected runs; incomplete batches are descriptive only.',
              'status': 'pilot-cell-pass' if not reasons else 'inconclusive',
              'attempts_sha256': sha(out/'attempts.json')}
    write(out/'verdict.json', result)
    return result


def native(args):
    root = args.root.resolve()
    reservation = read(args.window/'reservation.json')
    out = args.window.resolve()/'native-demo1-dense'
    out.mkdir()
    authorization = reservation.get('authorization') or {}
    if reservation.get('policy') != V4 or args.window.name not in (v4_windows(authorization) or [V4_WINDOW]):
        raise ValueError('native-window requires an accepted v4 validation reservation')
    plan = native_plan(root, out)
    plan['floors_ms'] = {'cpu_ms': args.cpu_floor_ms, 'emission_interval_ms': args.elapsed_floor_ms}
    if plan['floors_ms'] != {'cpu_ms':.01, 'emission_interval_ms':.01}:
        raise ValueError('v4 requires frozen 0.01 ms per-frame CPU and elapsed floors')
    plan.update(settle_seconds=90, reservation=reservation, kit_sha256=sha(root/'kit.json'))
    plan['warmup'] = {'full_prefix_runs': 1, 'command': [str(root/'bin/b1-perf'), 'sequence',
                      str(root/'captures/demo1-dense.f3dcap'), str(out/'warmup'), 'coarse']}
    write(out/'attempts.json', plan)
    try:
        verify_kit(root)
        probe = worker_probe(reservation)
        write(out/'worker.json', probe)
        if probe['errors']:
            raise ValueError('; '.join(probe['errors']))
        with (out/'driver-checks.log').open('x') as log:
            subprocess.run([sys.executable, str(root/'scripts/t1.py'), 'driver-checks',
                            '--binary', str(root/'bin/b1-perf'), '--out', str(out/'driver-checks')],
                           stdout=log, stderr=log, check=True)
        with (out/'warmup.log').open('x') as log:
            subprocess.run(plan['warmup']['command'], stdout=log, stderr=log, check=True)
        warmup_finished = utc()
        settle_start = time.monotonic()
        time.sleep(90)
        write(out/'settle.json', {'warmup_finished_utc': warmup_finished, 'seconds': 90,
              'elapsed_seconds': time.monotonic()-settle_start, 'finished_utc': utc()})
        with (out/'run.log').open('x') as log:
            for entry in plan['attempts']:
                end = datetime.fromisoformat(reservation['scheduled_end'].replace('Z', '+00:00'))
                if datetime.now(timezone.utc) >= end:
                    raise ValueError('reservation expired; remaining invocations are unlaunched')
                argv = [sys.executable, str(root/'scripts/protocol.py'), '--out', str(Path(entry['quiet_run']).parent),
                        '--attestation', reservation['operator_attestation'],
                        '--scheduled-start', reservation['scheduled_start'], '--scheduled-end', reservation['scheduled_end'],
                        '--allowed-idle', 'WindowServer', '--policy', V4, '--backend', 'native']
                for name in ['pair', 'revision', 'workload', 'configuration']:
                    argv.extend(['--'+name, entry[name]])
                argv += ['--', *entry['command']]
                log.write(json.dumps({'utc': utc(), 'argv': argv})+'\n')
                log.flush()
                subprocess.run(argv, stdout=log, stderr=log, check=False)
    except (Exception, KeyboardInterrupt) as error:
        write(out/'failure.json', {'error': str(error), 'utc': utc(), 'status': 'inconclusive'})
    result = audit(out)
    print(json.dumps(result, indent=2))
    return 0 if result['valid'] else 1


def main():
    parser = argparse.ArgumentParser(description='T1 parent-versus-parent kit; hardware runs require a named reservation')
    commands = parser.add_subparsers(dest='action', required=True)
    p = commands.add_parser('manifest')
    p.add_argument('--out', type=Path, required=True)
    p = commands.add_parser('freeze')
    p.add_argument('--root', type=Path, required=True)
    p = commands.add_parser('reserve')
    for name in ['root']:
        p.add_argument('--'+name, type=Path, required=True)
    for name in ['name', 'start', 'end', 'host', 'attestation']:
        p.add_argument('--'+name, required=True)
    p.add_argument('--policy', choices=[VERSION, V4], default=VERSION)
    p = commands.add_parser('native-window')
    for name in ['root', 'window']:
        p.add_argument('--'+name, type=Path, required=True)
    for name in ['cpu-floor-ms', 'elapsed-floor-ms']:
        p.add_argument('--'+name, type=float, required=True)
    p = commands.add_parser('driver-checks')
    for name in ['binary', 'out']:
        p.add_argument('--'+name, type=Path, required=True)
    p = commands.add_parser('browser-admit')
    p.add_argument('--workload', choices=['source', *CAPTURES], required=True)
    for name in ['coarse', 'readback', 'one-frame', 'out']:
        p.add_argument('--'+name, type=Path, required=True)
    p = commands.add_parser('decision')
    for name in ['root', 'out']:
        p.add_argument('--'+name, type=Path, required=True)
    args = parser.parse_args()
    if args.action == 'manifest':
        write(args.out, {'production_sha': PARENT, 'policies': {VERSION: POLICY, V4: V4_POLICY}, 'rows': row_manifest()})
    elif args.action == 'freeze':
        return freeze(args)
    elif args.action == 'reserve':
        print(reserve(args.root, args.name, args.start, args.end, args.host, args.attestation, args.policy))
    elif args.action == 'native-window':
        return native(args)
    elif args.action == 'driver-checks':
        driver_checks(args.binary, args.out)
    elif args.action == 'browser-admit':
        result = browser_admit(args.workload, args.coarse, args.readback, args.one_frame, args.out)
        return 0 if result['valid'] else 1
    elif args.action == 'decision':
        result = decision(args.root)
        write(args.out, result)
        return 0 if result['valid'] else 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
