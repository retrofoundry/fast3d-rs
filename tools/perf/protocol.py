#!/usr/bin/env python3
import argparse
import copy
import gzip
import hashlib
import json
import math
import os
import re
import statistics
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

VERSION = 'b2-quiet-v3'
V4 = 'b2-quiet-v4'
GPU_PRODUCERS = r'(Google Chrome|Chromium|chrome_crashpad_handler|Safari|Firefox|WebKit|MTLCompilerService|Metal|blender|OBS|QuickTime|VLC|Unity|Unreal|Godot|replay|sm64|helix)'
AGENTS = r'(^|/)(codex|claude)([ /]|$)|/claude/versions/|ChatGPT|Codex Framework'
PROHIBITED = r'(^|/)(cargo|rustc|rustup|ninja|make|cmake|clang(?:\+\+)?|cc1|ld|swiftc|wasm-bindgen|ffmpeg|rsync|curl|wget|replay_capture|sm64|helix)( |$)'
POLICY = {'resting_seconds': 30, 'preflight_seconds': 30, 'postflight_seconds': 10,
          'sample_period_seconds': 1, 'activity_window_samples': 30, 'maximum_gap_seconds': 1.5, 'spread_limit': .05,
          'idle_process_cores_average': .03, 'idle_process_cores_two_samples': .10,
          'resting_process_cores_average': .10, 'resting_process_cores_two_samples': .50,
          'background_margin_cores': .05, 'idle_margin_percentage_points': 1.0,
          'noise_mad_multiplier': 3, 'idle_percentile': .20, 'idle_trim_fraction': .10,
          'maximum_phase_transition_seconds': 3,
          'calibration_background_average_max': .35, 'calibration_background_two_samples_max': .75,
          'calibration_idle_percentile_min': 90,
          'calibration_background_mad_max': .10, 'calibration_idle_mad_max': 1.5,
          'reference_limits': 'background mean + max(0.05, 3*MAD); background two-sample ceiling = max + same margin; idle trimmed-mean floor = 10%-trimmed mean - max(1 percentage point, 3*MAD); independent absolute calibration limits apply before deriving allowances',
          'cpu_normalization': 'one fully occupied logical CPU = 1.0 core',
          'prohibited': PROHIBITED, 'gpu_producers': GPU_PRODUCERS, 'idle_agents': AGENTS,
          'idle_sessions': 'May remain open without work: each named agent/browser process must average <= 0.03 core in every 30-sample window (short runs include preflight history) and never exceed 0.10 core in two consecutive samples. Their CPU remains background.',
          'operator_obligations': 'Keep the host resting during calibration, then reserve it through postflight. Pause all agent tasks, builds, captures, transfers, browser interaction, GPU work and scheduled jobs. Idle sessions may remain under their activity ceilings. Attest to no unrelated GPU work.',
          'allowed_idle_matching': 'exact command or basename; applies idle activity ceilings, never overrides prohibited tools or exempts CPU',
          'monitor_exemptions': 'Only this monitor and its descendants, excluding the benchmark tree. No name-based exemption for top, ps, sysmond or Activity Monitor.',
          'first_observation': 'Initial inventory is CPU baseline only and archived; identity checks still apply. Later new PIDs are charged CPU since birth; start identity distinguishes PID reuse.',
          'monitor_version': VERSION,
          'gpu_activity': 'No portable per-process GPU utilization counter. Idle browser admission requires the operator attestation of no unrelated GPU work.'}
V4_POLICY = {**POLICY, 'monitor_version': V4,
             'scope': 'native.demo1-dense.coarse',
             'reference_limits': POLICY['reference_limits'] + '; v4 overrides the preflight/run/postflight background burst, sustained and idle thresholds with fixed values; every other ceiling with a fixed 0.75 core',
             'measurement_background_two_samples_max': .75,
             # Fixed like the burst ceiling and for the same reason: these were derived from each
             # invocation's own resting calibration, so the quieter the host, the stricter the gate
             # its own run then had to meet. Window 5 on ci8 rejected three invocations that way,
             # against thresholds of 0.0774/0.0801 core and a 96.80% idle floor, while every spread
             # passed. Accepted by David 2026-09-13.
             'measurement_background_average_max': .15,
             'measurement_minimum_idle_percent': 96.0,
             'observed_spread_limits': {'cpu_ms': .25, 'emission_interval_ms': .15},
             'cold_spread_limit': .05, 'absolute_floor_ms_per_frame': .01,
             'pairs': 5, 'pair_order': ['AB', 'BA', 'AB', 'BA', 'AB'],
             'throwaway_full_prefix_warmups': 1, 'settle_seconds': 90,
             'component_spreads': 'diagnostic',
             'directional_rule': 'All five candidate-minus-parent deltas positive and median > max(parent range, candidate range, floor) blocks admission; all negative with savings median > uncertainty resolves a speedup. Identical-binary validation rejects either direction.'}


def timing_policy(version=VERSION, backend=None, workload=None, configuration=None):
    if version == V4:
        if (backend, workload, configuration) != ('native', 'demo1-dense', 'coarse'):
            raise ValueError('b2-quiet-v4 applies only to native.demo1-dense coarse timing')
        return V4_POLICY
    if version != VERSION:
        raise ValueError(f'unknown timing policy: {version}')
    return POLICY


COMMANDS = {'processes': ['ps', '-ww', '-axo', 'pid=,ppid=,lstart=,time=,comm='],
            'cpu': ['top', '-l', '0', '-s', '1', '-n', '0'],
            'memory': ['vm_stat'], 'thermal': ['pmset', '-g', 'therm'],
            'power': ['pmset', '-g', 'batt'], 'power_mode': ['pmset', '-g', 'custom']}

def utc():
    return datetime.now(timezone.utc).isoformat()

def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()

def command(args, cwd=None):
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, timeout=30,
                            env={**os.environ, 'LC_ALL':'C'})
    if result.returncode:
        raise RuntimeError(f'{args}: {result.stderr}')
    return result.stdout

def cpu_seconds(value):
    days, value = value.split('-', 1) if '-' in value else ('0', value)
    fields = value.split(':')
    total = 0.0
    for field in fields:
        total = total * 60 + float(field)
    return total + int(days) * 86400

def inventory(raw):
    processes = {}
    for line in raw.splitlines():
        fields = line.strip().split(maxsplit=8)
        if fields[2][0].isalpha():
            pid, ppid = fields[:2]
            started = ' '.join(fields[2:7])
            elapsed, name = fields[7:]
        else:
            pid, ppid, elapsed, name = line.strip().split(maxsplit=3)
            started = None
        processes[int(pid)] = {'pid':int(pid), 'ppid':int(ppid), 'cpu_seconds':cpu_seconds(elapsed),
                               'command':name, 'started':started}
    return processes

def descendants(processes, roots):
    result = set(roots)
    while True:
        children = {p['pid'] for p in processes.values() if p['ppid'] in result}
        if children <= result:
            return result
        result |= children

def account_processes(processes, previous, duration, benchmark, monitor):
    for pid, process in processes.items():
        old = (previous or {}).get(pid)
        same = old and old.get('started') == process.get('started') and process['cpu_seconds'] >= old['cpu_seconds']
        if previous is None:
            process['cores'] = None
        else:
            process['cores'] = (process['cpu_seconds'] - old['cpu_seconds'] if same else process['cpu_seconds']) / duration
        process['exemption'] = 'benchmark' if pid in benchmark else 'monitor' if pid in monitor else None


def recalculate(samples):
    corrected = []
    for previous, original in zip(samples, samples[1:]):
        sample = copy.deepcopy(original)
        processes = {p['pid']: p for p in sample['processes']}
        old = {p['pid']: p for p in previous['processes']}
        benchmark = {p['pid'] for p in processes.values() if p.get('exemption') == 'benchmark'}
        monitor = {p['pid'] for p in processes.values() if p.get('exemption') == 'monitor'}
        account_processes(processes, old, sample['monotonic'] - previous['monotonic'], benchmark, monitor)
        sample['background_cores'] = sum(p['cores'] or 0 for p in processes.values() if not p['exemption'])
        sample['disallowed'] = []
        corrected.append(sample)
    return corrected


def idle_process(name, allowed_idle):
    if name.endswith('/com.apple.Safari.SafeBrowsing.Service'):
        return name in allowed_idle or Path(name).name in allowed_idle
    return bool(re.search(AGENTS, name, re.I) or re.search(GPU_PRODUCERS, name, re.I)
                or name in allowed_idle or Path(name).name in allowed_idle)


def maximum_average(values):
    window = min(len(values), POLICY['activity_window_samples'])
    if not window:
        return 0.0
    total = sum(values[:window])
    maximum = total
    for index in range(window, len(values)):
        total += values[index] - values[index-window]
        maximum = max(maximum, total)
    return maximum / window


def trimmed_mean(values):
    values = sorted(values)
    trim = int(len(values)*POLICY['idle_trim_fraction'])
    return statistics.mean(values[trim:len(values)-trim])


def percentile(values, fraction):
    values = sorted(values)
    position = (len(values)-1)*fraction
    lower = int(position)
    upper = min(lower+1, len(values)-1)
    return values[lower] + (values[upper]-values[lower])*(position-lower)


def process_activity(samples, phase, allowed_idle=()):
    rows = {}
    reasons = []
    for index, sample in enumerate(samples):
        for p in sample.get('processes', []):
            name = p['command']
            exemption = p.get('exemption')
            if re.search(PROHIBITED, name, re.I) and exemption != 'monitor':
                reasons.append(f"disallowed job: {name} (pid {p['pid']}; present)")
            if exemption == 'benchmark' and sample.get('phase', phase) == 'postflight':
                reasons.append(f"benchmark process survived teardown: {name} (pid {p['pid']})")
            if exemption:
                continue
            key = (p['pid'], p.get('started'), name)
            if key not in rows:
                rows[key] = {'pid':p['pid'], 'command':name, 'cores':[0.0]*len(samples),
                             'idle_policy':idle_process(name, allowed_idle)}
            rows[key]['cores'][index] = p['cores'] or 0
    reports = []
    for row in rows.values():
        values = row.pop('cores')
        average = statistics.mean(values)
        activity_average = maximum_average(values)
        sustained = max((min(a,b) for a,b in zip(values,values[1:])), default=0)
        row.update(average_cores=average, maximum_average_cores=activity_average, maximum_cores=max(values), maximum_two_sample_cores=sustained)
        if row['idle_policy'] or phase == 'resting':
            category = 'idle_process' if row['idle_policy'] else 'resting_process'
            mean_limit = POLICY[category+'_cores_average']
            peak_limit = POLICY[category+'_cores_two_samples']
            label = f"{row['command']} (pid {row['pid']})"
            if activity_average > mean_limit:
                reasons.append(f'active process: {label}; activity average {activity_average:.4f} core > {mean_limit:.2f}')
            if sustained > peak_limit:
                reasons.append(f'active process: {label}; two consecutive samples > {peak_limit:.2f} core')
        reports.append(row)
    return reports, sorted(set(reasons))


def resting_profile(samples, allowed_idle=()):
    def distribution(field):
        values = [s[field] for s in samples]
        middle = statistics.median(values)
        return {'minimum':min(values), 'median':middle, 'mean':statistics.mean(values),
                'maximum':max(values), 'trimmed_mean':trimmed_mean(values), 'p20':percentile(values, POLICY['idle_percentile']), 'mad':statistics.median(abs(v-middle) for v in values)}
    background = distribution('background_cores')
    idle = distribution('idle_percent')
    background_margin = max(POLICY['background_margin_cores'], POLICY['noise_mad_multiplier']*background['mad'])
    idle_margin = max(POLICY['idle_margin_percentage_points'], POLICY['noise_mad_multiplier']*idle['mad'])
    processes, reasons = process_activity(samples, 'resting', allowed_idle)
    checks = [
        (background['mean'], 'calibration_background_average_max', 'background average', False),
        (max((min(a['background_cores'], b['background_cores']) for a,b in zip(samples,samples[1:])), default=0),
         'calibration_background_two_samples_max', 'background two samples', False),
        (idle['p20'], 'calibration_idle_percentile_min', 'idle p20', True),
        (background['mad'], 'calibration_background_mad_max', 'background MAD', False),
        (idle['mad'], 'calibration_idle_mad_max', 'idle MAD', False),
    ]
    for value, key, label, lower in checks:
        limit = POLICY[key]
        if (value < limit) if lower else (value > limit):
            reasons.append(f'dirty calibration: {label} {value:.4f} {"<" if lower else ">"} {limit}')
    return {'sample_count':len(samples), 'utc_interval':[samples[0].get('utc'),samples[-1].get('utc')],
            'background_cores':background, 'idle_percent':idle,
            'thresholds':{'maximum_background_cores_average':background['mean']+background_margin,
                          'maximum_background_cores_two_samples':background['maximum']+background_margin,
                          'minimum_idle_percent':max(POLICY['calibration_idle_percentile_min'], idle['trimmed_mean']-idle_margin)},
            'idle_processes':[p for p in processes if p['idle_policy']],
            'process_activity':processes, 'reasons':reasons}


def sample_reasons(samples, phase, profile=None, allowed_idle=(), history=(), version=VERSION):
    reasons = []
    if not samples:
        return ['missing telemetry']
    seconds = POLICY.get(phase+'_seconds')
    if seconds and (len(samples) < seconds or samples[-1]['monotonic'] - samples[0]['monotonic'] < seconds-1-(POLICY['maximum_gap_seconds']-POLICY['sample_period_seconds'])):
        reasons.append(f'incomplete {seconds}-second {phase}')
    activity = list(history[-(POLICY['activity_window_samples']-1):]) + samples
    thresholds = dict(profile['thresholds']) if profile else None
    if thresholds and version == V4 and phase in ['preflight', 'run', 'postflight']:
        thresholds['maximum_background_cores_two_samples'] = V4_POLICY['measurement_background_two_samples_max']
        thresholds['maximum_background_cores_average'] = V4_POLICY['measurement_background_average_max']
        thresholds['minimum_idle_percent'] = V4_POLICY['measurement_minimum_idle_percent']
    if phase != 'resting' and thresholds is None:
        reasons.append('missing resting profile')
    if profile:
        reasons.extend(profile['reasons'])
    for previous, current in zip(activity, activity[1:]):
        gap = current['monotonic'] - previous['monotonic']
        transition = previous.get('phase') != current.get('phase')
        limit = POLICY['maximum_phase_transition_seconds'] if transition else POLICY['maximum_gap_seconds']
        if gap <= 0 or gap > limit:
            reasons.append('phase transition gap' if transition else 'monitor gap')
        if current['swapouts'] > previous['swapouts']:
            reasons.append('swap-out growth')
        if current['power'] != previous['power'] or current['power_mode'] != previous['power_mode']:
            reasons.append('power source or mode changed')
        if thresholds and min(previous['background_cores'], current['background_cores']) > thresholds['maximum_background_cores_two_samples']:
            reasons.append(f"background above resting ceiling {thresholds['maximum_background_cores_two_samples']:.4f} core twice")
    if thresholds and maximum_average([s['background_cores'] for s in activity]) > thresholds['maximum_background_cores_average']:
        reasons.append(f"background average above resting allowance {thresholds['maximum_background_cores_average']:.4f} core")
    if thresholds and phase != 'run':
        idle = trimmed_mean([s['idle_percent'] for s in samples])
        if idle < thresholds['minimum_idle_percent']:
            reasons.append(f"{phase} idle trimmed mean {idle:.2f}% below resting floor {thresholds['minimum_idle_percent']:.2f}%")
    for sample in samples:
        if sample.get('error'):
            reasons.append('monitor failure')
        if sample['thermal_throttled']:
            reasons.append('thermal throttling')
        if sample['idle_age_seconds'] > POLICY['maximum_gap_seconds']:
            reasons.append('CPU telemetry gap')
        if 'processes' not in sample:
            reasons.append('missing process inventory')
    reasons += process_activity(activity, phase, allowed_idle)[1]
    return sorted(set(reasons))


def replay(samples, allowed_idle=()):
    corrected = recalculate(samples)
    profile = resting_profile(corrected, allowed_idle)
    reasons = sample_reasons(corrected, 'replay', profile, allowed_idle)
    baseline = copy.deepcopy(samples[:1])
    for sample in baseline:
        for process in sample['processes']:
            process['cores'] = None
    reasons = sorted(set(reasons + process_activity(baseline, 'baseline', allowed_idle)[1]))
    return {'quiet_protocol_version':VERSION, 'quiet_valid':not reasons, 'reasons':reasons,
            'timing_valid':False, 'usable_intervals':len(corrected), 'resting_profile':profile,
            'scope':'Retrospective quiet check only: first snapshot is the CPU baseline; remaining intervals are reused as the resting reference. No independent calibration or benchmark was recorded.'}

def replay_attempt(samples, record=None, version=VERSION, backend=None):
    record = record or {}
    timing_policy(version, backend or record.get('backend'), record.get('workload'), record.get('configuration'))
    allowed = record.get('allowed_idle_services', [])
    corrected = recalculate(samples)
    resting = [s for s in corrected if s['phase']=='resting']
    if not resting:
        return {'quiet_protocol_version':version, 'quiet_valid':False, 'complete':False,
                'timing_valid':False, 'usable_intervals':len(corrected),
                'phase_reasons':{}, 'reasons':['missing resting telemetry']}
    profile = resting_profile(resting, allowed)
    reasons = process_activity(samples[:1], 'baseline', allowed)[1]
    phase_reasons = {}
    for phase in ['resting', 'preflight', 'run', 'postflight']:
        selected = [s for s in corrected if s['phase']==phase]
        if not selected:
            continue
        before = [s for s in corrected if s['monotonic'] < selected[0]['monotonic']]
        phase_reasons[phase] = sample_reasons(selected, phase, profile, allowed,
                                             before if phase != 'resting' else [], version)
        reasons.extend(phase_reasons[phase])
    complete = (record.get('exit_code') == 0 and
                all(any(s['phase']==phase for s in corrected) for phase in ['preflight','run','postflight']))
    if not complete:
        reasons.append('benchmark not completed; no full quiet run')
    reasons = sorted(set(reasons))
    return {'quiet_protocol_version':version, 'quiet_valid':not reasons, 'complete':complete,
            'reasons':reasons, 'phase_reasons':phase_reasons, 'resting_profile':profile,
            'timing_valid':False, 'usable_intervals':len(corrected),
            'scope':'Retrospective telemetry verdict; timing artifacts and reservation are not re-admitted.'}


def spread(values, limit=.05):
    if not values or any(not isinstance(v, (int,float)) or not math.isfinite(v) or v <= 0 for v in values):
        return {'valid':False, 'reason':'requires five finite positive totals', 'values':values, 'limit':limit}
    middle = statistics.median(values)
    width = max(values) - min(values)
    ratio = width / middle
    return {'valid':len(values) == 5 and (ratio <= limit or math.isclose(ratio, limit, rel_tol=1e-12)), 'values':values, 'median':middle,
            'absolute_range':width, 'relative_spread':ratio, 'limit':limit}

def invalidate_pairs(runs):
    invalid = {r['pair'] for r in runs if not r['valid']}
    for run in runs:
        if run['pair'] in invalid:
            run['valid'] = False
            if not run['reasons']:
                run['reasons'] = ['paired repetition invalid']
    return runs

def disallowed_process(process, allowed_idle):
    return bool(re.search(PROHIBITED, process['command'], re.I) or
                (idle_process(process['command'], allowed_idle) and
                 (process['cores'] or 0) > POLICY['idle_process_cores_average']))

class Monitor:
    def __init__(self, allowed_idle):
        self.allowed_idle = allowed_idle
        self.benchmark_pids = {}
        self.benchmark_root = None
        self.benchmark_process = None
        self.last = None
        self.idle = None
        self.top = subprocess.Popen(COMMANDS['cpu'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        self.top_raw = []
        self.reader = threading.Thread(target=self.read_top, daemon=True)
        self.reader.start()
    def read_top(self):
        for line in self.top.stdout:
            self.top_raw.append({'utc':utc(), 'line':line})
            match = re.search(r'([\d.]+)% idle', line)
            if match:
                self.idle = (time.monotonic(), float(match[1]))
    def take(self, phase):
        start = time.monotonic()
        raw = {key:command(args) for key,args in COMMANDS.items() if key != 'cpu'}
        processes = inventory(raw['processes'])
        now = time.monotonic()
        previous_time, previous = self.last or (now - 1, None)
        duration = now - previous_time
        roots = {pid for pid, started in self.benchmark_pids.items()
                 if pid in processes and processes[pid]['started'] == started}
        if self.benchmark_process is not None and self.benchmark_process.poll() is None:
            roots.add(self.benchmark_root)
        required = descendants(processes, roots)
        self.benchmark_pids = {pid:processes[pid]['started'] for pid in required & processes.keys()}
        monitor = descendants(processes, {os.getpid()}) - required
        account_processes(processes, previous, duration, required, monitor)
        self.last = (now, copy.deepcopy(processes))
        background = sum(p['cores'] or 0 for p in processes.values() if not p['exemption'])
        disallowed = [{'pid':p['pid'], 'command':p['command'], 'reason':'prohibited tool present'}
                      for p in processes.values() if p['exemption'] != 'monitor' and re.search(PROHIBITED,p['command'],re.I)]
        swaps = re.search(r'Swapouts:\s+(\d+)', raw['memory'])
        if not swaps or self.idle is None:
            raise RuntimeError('missing swap or aggregate idle telemetry')
        limits = [int(v) for v in re.findall(r'(?:CPU_Speed_Limit|CPU_Scheduler_Limit)\s*=\s*(\d+)', raw['thermal'])]
        warning = re.search(r'(?:Thermal|Performance)\w*\s*(?:=|:)\s*([1-9]\d*)',raw['thermal'],re.I)
        return {'utc':utc(), 'monotonic':now, 'phase':phase, 'idle_percent':self.idle[1], 'idle_age_seconds':now-self.idle[0],
                'background_cores':background, 'processes':list(processes.values()), 'disallowed':disallowed,
                'swapouts':int(swaps[1]), 'thermal_throttled':any(v<100 for v in limits) or bool(warning),
                'power':raw['power'].splitlines()[0], 'power_mode':raw['power_mode'], 'raw':raw, 'monitor_elapsed_ms':(now-start)*1000}
    def close(self):
        self.top.terminate()
        try:
            self.top.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.top.kill()
            self.top.wait(timeout=5)
        self.reader.join(timeout=5)
        self.top.stdout.close()
        self.top.stderr.close()

def quiet(args):
    version = getattr(args, 'policy', VERSION)
    policy = timing_policy(version, getattr(args, 'backend', None), args.workload, args.configuration)
    out = args.out
    out.mkdir(parents=True, exist_ok=False)
    record = {'quiet_protocol_version':version, 'backend':getattr(args, 'backend', None), 'scheduled_utc_interval':[args.scheduled_start,args.scheduled_end],
              'monitoring_utc_interval':[utc(),None], 'actual_utc_interval':[None,None],
              'operator_attestation':args.attestation, 'allowed_idle_services':args.allowed_idle,
              'policy':policy, 'monitor_commands':COMMANDS, 'command':args.command, 'pair':args.pair, 'revision':args.revision, 'workload':args.workload, 'configuration':args.configuration,
              'monitor_environment':{'LC_ALL':'C'}, 'valid':False, 'reasons':[], 'exit_code':None, 'resting_profile':None}
    samples = []
    monitor = None
    def phase_context(phase):
        selected = [s for s in samples if s['phase']==phase]
        before = [s for s in samples if s['phase'] != 'baseline' and selected and s['monotonic'] < selected[0]['monotonic']]
        return selected, before[-(POLICY['activity_window_samples']-1):]
    try:
        monitor = Monitor(args.allowed_idle)
        with (out/'telemetry.jsonl').open('w') as stream:
            next_sample = time.monotonic()+2
            def take(phase):
                nonlocal next_sample
                time.sleep(max(0, next_sample-time.monotonic()))
                next_sample = time.monotonic()+POLICY['sample_period_seconds']
                sample = monitor.take(phase)
                samples.append(sample)
                stream.write(json.dumps(sample)+'\n')
                stream.flush()
            take('baseline')
            baseline_reasons = process_activity(samples, 'baseline', args.allowed_idle)[1]
            for _ in range(POLICY['resting_seconds']):
                take('resting')
            resting = [s for s in samples if s['phase']=='resting']
            profile = resting_profile(resting, args.allowed_idle)
            profile['reasons'] = sorted(set(profile['reasons'] + sample_reasons(resting, 'resting', allowed_idle=args.allowed_idle)))
            (out/'resting-profile.json').write_text(json.dumps(profile,indent=2))
            record['resting_profile'] = {'path':str(out/'resting-profile.json'), 'sha256':sha(out/'resting-profile.json'),
                                         'thresholds':profile['thresholds'], 'idle_processes':profile['idle_processes']}
            record['reasons'] = sorted(set(profile['reasons'] + baseline_reasons))
            def validate(phase):
                phase_samples, before = phase_context(phase)
                return sample_reasons(phase_samples, phase, profile, args.allowed_idle, before, version)
            if not record['reasons']:
                record['actual_utc_interval'][0] = utc()
                for _ in range(POLICY['preflight_seconds']):
                    take('preflight')
                record['reasons'] = validate('preflight')
            if not record['reasons']:
                with (out/'stdout.log').open('w') as stdout, (out/'stderr.log').open('w') as stderr:
                    process = subprocess.Popen(args.command, stdout=stdout, stderr=stderr)
                    monitor.benchmark_root = process.pid
                    monitor.benchmark_process = process
                    record['benchmark_pid'] = process.pid
                    while process.poll() is None:
                        take('run')
                    record['exit_code'] = process.returncode
                    if process.returncode:
                        record['reasons'].append('benchmark failed')
                for _ in range(POLICY['postflight_seconds']):
                    take('postflight')
                for phase in ['run','postflight']:
                    record['reasons'] += validate(phase)
    except Exception as error:
        record['reasons'].append(str(error))
    finally:
        if monitor is not None:
            try:
                monitor.close()
            except Exception as error:
                record['reasons'].append(f'monitor shutdown failed: {error}')
            (out/'top.json').write_text(json.dumps(monitor.top_raw))
        record['monitoring_utc_interval'][1] = utc()
        if record['actual_utc_interval'][0]:
            record['actual_utc_interval'][1] = record['monitoring_utc_interval'][1]
        try:
            planned_start, planned_end = (datetime.fromisoformat(v.replace('Z','+00:00')) for v in record['scheduled_utc_interval'])
            if record['actual_utc_interval'][0]:
                actual_start, actual_end = (datetime.fromisoformat(v) for v in record['actual_utc_interval'])
                if not planned_start <= actual_start <= actual_end <= planned_end:
                    record['reasons'].append('run outside reserved interval')
        except (ValueError, TypeError):
            record['reasons'].append('invalid reserved UTC interval')
        record['process_activity'] = {}
        record['activity_history_samples'] = {}
        for phase in ['resting','preflight','run','postflight']:
            selected, before = phase_context(phase)
            record['process_activity'][phase] = process_activity(before+selected, phase, args.allowed_idle)[0]
            record['activity_history_samples'][phase] = len(before)
        record['reasons'] = sorted(set(record['reasons']))
        record['valid'] = not record['reasons'] and record['exit_code'] == 0
        record['telemetry'] = [{'path':str(out/name), 'sha256':sha(out/name)} for name in ['telemetry.jsonl','top.json','resting-profile.json'] if (out/name).exists()]
        (out/'run.json').write_text(json.dumps(record,indent=2))
    print(json.dumps({'quiet_protocol_version':version, 'valid':record['valid'],'reasons':record['reasons']}))
    return 0 if record['valid'] else 1

def main():
    if sys.argv[1:2] == ['replay']:
        parser = argparse.ArgumentParser(description='Retrospective quiet check; never admits timing evidence')
        parser.add_argument('--telemetry', type=Path, required=True)
        parser.add_argument('--out', type=Path, required=True)
        parser.add_argument('--allowed-idle', action='append', default=[])
        parser.add_argument('--run', type=Path, help='Replay phase-aware attempt telemetry with its original run.json')
        parser.add_argument('--policy', choices=[VERSION, V4], default=VERSION)
        parser.add_argument('--backend', choices=['native', 'chrome'])
        args = parser.parse_args(sys.argv[2:])
        opener = gzip.open if args.telemetry.suffix == '.gz' else open
        with opener(args.telemetry, 'rt') as stream:
            samples = [json.loads(line) for line in stream]
        if args.policy == V4 and not args.run:
            parser.error('v4 replay requires a scoped run record')
        record = json.loads(args.run.read_text()) if args.run else {}
        result = replay_attempt(samples, record, args.policy, args.backend) if args.run else replay(samples, args.allowed_idle)
        result.update(policy=timing_policy(args.policy, args.backend or record.get('backend'), record.get('workload'), record.get('configuration')), allowed_idle_services=args.allowed_idle,
                      source={'path':str(args.telemetry), 'sha256':sha(args.telemetry)})
        with args.out.open('x') as stream:
            json.dump(result, stream, indent=2)
        print(json.dumps({k:result[k] for k in ['quiet_valid','timing_valid','reasons','usable_intervals']}))
        return 0 if result['quiet_valid'] else 1
    parser = argparse.ArgumentParser()
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--attestation',required=True)
    parser.add_argument('--scheduled-start',required=True)
    parser.add_argument('--scheduled-end',required=True)
    parser.add_argument('--allowed-idle',action='append',default=[])
    parser.add_argument('--pair',required=True)
    parser.add_argument('--revision',required=True)
    parser.add_argument('--workload',required=True)
    parser.add_argument('--configuration',required=True)
    parser.add_argument('--policy', choices=[VERSION, V4], default=VERSION)
    parser.add_argument('--backend', choices=['native', 'chrome'])
    parser.add_argument('command',nargs=argparse.REMAINDER)
    args=parser.parse_args()
    if args.command[:1]==['--']:
        args.command=args.command[1:]
    return quiet(args)

if __name__ == '__main__':
    raise SystemExit(main())
