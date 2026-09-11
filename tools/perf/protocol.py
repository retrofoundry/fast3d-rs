#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import re
import statistics
import subprocess
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

VERSION = 'b1-quiet-v1'
GPU_PRODUCERS = r'(Google Chrome|Chromium|Safari|Firefox|WebKit|Metal|blender|OBS|QuickTime|VLC|Unity|Unreal|Godot|replay|sm64|helix)'
PROHIBITED = r'(^|/)(codex|claude|cargo|rustc|ninja|make|ffmpeg|rsync|curl|wget|replay_capture|sm64|helix)( |$)'
POLICY = {'preflight_seconds': 30, 'postflight_seconds': 10, 'sample_period_seconds': 1,
          'minimum_preflight_idle_percent': 95, 'maximum_background_cores_average': .10,
          'maximum_background_cores_two_samples': .25, 'maximum_gap_seconds': 1.5,
          'spread_limit': .05, 'cpu_normalization': 'one fully occupied logical CPU = 1.0 core',
          'prohibited': PROHIBITED, 'gpu_producers': GPU_PRODUCERS,
          'allowed_idle_matching': 'exact command or basename; CPU remains charged to background', 'monitor_version': VERSION,
          'gpu_activity': 'No portable per-process GPU utilization counter; full process inventory checked for unrelated GPU-producing jobs.'}
COMMANDS = {'processes': ['ps', '-axo', 'pid=,ppid=,time=,comm='],
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
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, timeout=30)
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
        pid, ppid, elapsed, name = line.strip().split(maxsplit=3)
        processes[int(pid)] = {'pid':int(pid), 'ppid':int(ppid), 'cpu_seconds':cpu_seconds(elapsed), 'command':name}
    return processes

def descendants(processes, roots):
    result = set(roots)
    while True:
        children = {p['pid'] for p in processes.values() if p['ppid'] in result}
        if children <= result:
            return result
        result |= children

def sample_reasons(samples, phase):
    reasons = []
    if not samples:
        return ['missing telemetry']
    if phase == 'preflight' and (len(samples) < 30 or samples[-1]['monotonic'] - samples[0]['monotonic'] < 29):
        reasons.append('incomplete 30-second preflight')
    if phase == 'postflight' and (len(samples) < 10 or samples[-1]['monotonic'] - samples[0]['monotonic'] < 9):
        reasons.append('incomplete 10-second postflight')
    for previous, current in zip(samples, samples[1:]):
        if current['monotonic'] - previous['monotonic'] > POLICY['maximum_gap_seconds']:
            reasons.append('monitor gap')
        if current['swapouts'] > previous['swapouts']:
            reasons.append('swap-out growth')
        if current['power'] != previous['power'] or current['power_mode'] != previous['power_mode']:
            reasons.append('power source or mode changed')
        if phase == 'run' and min(previous['background_cores'], current['background_cores']) > .25:
            reasons.append('background above 0.25 core twice')
    if phase == 'run' and statistics.mean(s['background_cores'] for s in samples) > .10:
        reasons.append('background average above 0.10 core')
    for sample in samples:
        if sample.get('error'):
            reasons.append('monitor failure')
        if sample['thermal_throttled']:
            reasons.append('thermal throttling')
        if sample['disallowed']:
            reasons.append('disallowed active job')
        if phase == 'preflight' and sample['idle_percent'] < 95:
            reasons.append('preflight idle below 95%')
        if sample['idle_age_seconds'] > POLICY['maximum_gap_seconds']:
            reasons.append('CPU telemetry gap')
    return sorted(set(reasons))

def spread(values):
    if len(values) != 5 or any(not isinstance(v, (int,float)) or v <= 0 for v in values):
        return {'valid':False, 'reason':'requires five positive totals', 'values':values}
    middle = statistics.median(values)
    width = max(values) - min(values)
    ratio = width / middle
    return {'valid':ratio <= .05, 'values':values, 'median':middle, 'absolute_range':width, 'relative_spread':ratio}

def invalidate_pairs(runs):
    invalid = {r['pair'] for r in runs if not r['valid']}
    for run in runs:
        if run['pair'] in invalid:
            run['valid'] = False
            if not run['reasons']:
                run['reasons'] = ['paired repetition invalid']
    return runs

def disallowed_process(process, allowed_idle):
    name = process['command']
    active_prohibited = bool(re.search(PROHIBITED, name, re.I)) and process['cores'] > .005
    gpu = bool(re.search(GPU_PRODUCERS, name, re.I))
    declared_idle = name in allowed_idle or Path(name).name in allowed_idle
    return active_prohibited or (gpu and not (declared_idle and process['cores'] <= .005))

class Monitor:
    def __init__(self, allowed_idle):
        self.allowed_idle = allowed_idle
        self.roots = {os.getpid()}
        self.benchmark_root = None
        self.last = None
        self.idle = None
        self.top = subprocess.Popen(COMMANDS['cpu'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        self.top_raw = []
        threading.Thread(target=self.read_top, daemon=True).start()
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
        previous_time, previous = self.last or (start - 1, {})
        duration = now - previous_time
        self.last = (now, processes)
        required = descendants(processes, {self.benchmark_root} if self.benchmark_root else set())
        monitor = descendants(processes, {os.getpid()}) - required
        background = 0
        disallowed = []
        for pid, process in processes.items():
            old = previous.get(pid)
            process['cores'] = max(0, process['cpu_seconds'] - old['cpu_seconds']) / duration if old else process['cpu_seconds'] / duration
            process['exemption'] = 'benchmark' if pid in required else 'monitor' if pid in monitor else None
            if pid in required and re.search(PROHIBITED, process['command'], re.I):
                disallowed.append(pid)
            if not process['exemption']:
                background += process['cores']
                if disallowed_process(process, self.allowed_idle):
                    disallowed.append(pid)
            if pid not in previous and self.last and re.search(PROHIBITED, process['command'], re.I) and not process['exemption']:
                disallowed.append(pid)
        swaps = re.search(r'Swapouts:\s+(\d+)', raw['memory'])
        if not swaps or self.idle is None:
            raise RuntimeError('missing swap or aggregate idle telemetry')
        limits = [int(v) for v in re.findall(r'(?:CPU_Speed_Limit|CPU_Scheduler_Limit)\s*=\s*(\d+)', raw['thermal'])]
        warning = re.search(r'(?:Thermal|Performance)\w*\s*(?:=|:)\s*([1-9]\d*)',raw['thermal'],re.I)
        return {'utc':utc(), 'monotonic':now, 'phase':phase, 'idle_percent':self.idle[1], 'idle_age_seconds':now-self.idle[0],
                'background_cores':background, 'processes':list(processes.values()), 'disallowed':sorted(set(disallowed)),
                'swapouts':int(swaps[1]), 'thermal_throttled':any(v<100 for v in limits) or bool(warning),
                'power':raw['power'].splitlines()[0], 'power_mode':raw['power_mode'], 'raw':raw, 'monitor_elapsed_ms':(now-start)*1000}
    def close(self):
        self.top.terminate()
        self.top.wait()

def quiet(args):
    out = args.out
    out.mkdir(parents=True, exist_ok=False)
    record = {'quiet_protocol_version':VERSION, 'scheduled_utc_interval':[args.scheduled_start,args.scheduled_end],
              'actual_utc_interval':[utc(),None], 'operator_attestation':args.attestation, 'allowed_idle_services':args.allowed_idle,
              'policy':POLICY, 'monitor_commands':COMMANDS, 'command':args.command, 'pair':args.pair, 'revision':args.revision, 'workload':args.workload, 'configuration':args.configuration,
              'valid':False, 'reasons':[], 'exit_code':None}
    samples = []
    monitor = None
    try:
        monitor = Monitor(args.allowed_idle)
        with (out/'telemetry.jsonl').open('w') as stream:
            def take(phase):
                deadline = time.monotonic()+1
                sample = monitor.take(phase)
                samples.append(sample)
                stream.write(json.dumps(sample)+'\n')
                stream.flush()
                time.sleep(max(0,deadline-time.monotonic()))
            time.sleep(2)
            monitor.take('baseline')
            for _ in range(30):
                take('preflight')
            record['reasons'] = sample_reasons(samples, 'preflight')
            if not record['reasons']:
                with (out/'stdout.log').open('w') as stdout, (out/'stderr.log').open('w') as stderr:
                    process = subprocess.Popen(args.command, stdout=stdout, stderr=stderr)
                    monitor.benchmark_root = process.pid
                    record['benchmark_pid'] = process.pid
                    while process.poll() is None:
                        take('run')
                    record['exit_code'] = process.returncode
                    if process.returncode:
                        record['reasons'].append('benchmark failed')
                for _ in range(10):
                    take('postflight')
                for phase in ['run','postflight']:
                    record['reasons'] += sample_reasons([s for s in samples if s['phase']==phase], phase)
                for a,b in zip(samples,samples[1:]):
                    if b['swapouts'] > a['swapouts']:
                        record['reasons'].append('swap-out growth across phase boundary')
                    if a['power'] != b['power'] or a['power_mode'] != b['power_mode']:
                        record['reasons'].append('power changed across phase boundary')
                    if b['monotonic'] - a['monotonic'] > POLICY['maximum_gap_seconds']:
                        record['reasons'].append('monitor gap across phase boundary')
    except Exception as error:
        record['reasons'].append(str(error))
    finally:
        if monitor is not None:
            monitor.close()
            (out/'top.json').write_text(json.dumps(monitor.top_raw))
        record['actual_utc_interval'][1] = utc()
        try:
            planned_start, planned_end = (datetime.fromisoformat(v.replace('Z','+00:00')) for v in record['scheduled_utc_interval'])
            actual_start, actual_end = (datetime.fromisoformat(v) for v in record['actual_utc_interval'])
            if not planned_start <= actual_start <= actual_end <= planned_end:
                record['reasons'].append('run outside reserved interval')
        except (ValueError, TypeError):
            record['reasons'].append('invalid reserved UTC interval')
        record['reasons'] = sorted(set(record['reasons']))
        record['valid'] = not record['reasons'] and record['exit_code'] == 0
        record['telemetry'] = [{'path':str(out/name), 'sha256':sha(out/name)} for name in ['telemetry.jsonl','top.json'] if (out/name).exists()]
        (out/'run.json').write_text(json.dumps(record,indent=2))
    print(json.dumps({'valid':record['valid'],'reasons':record['reasons']}))
    return 0 if record['valid'] else 1

if __name__ == '__main__':
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
    parser.add_argument('command',nargs=argparse.REMAINDER)
    args=parser.parse_args()
    if args.command[:1]==['--']:
        args.command=args.command[1:]
    raise SystemExit(quiet(args))
