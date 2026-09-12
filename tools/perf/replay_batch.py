#!/usr/bin/env python3
import argparse
import collections
import json
from pathlib import Path

from protocol import VERSION, V4, replay_attempt, sha, timing_policy
from report import batch_metrics


def replay_directory(attempt, out, version=VERSION, backend=None):
    out.mkdir(parents=True, exist_ok=False)
    results = []
    runs = []
    summaries = {}
    for path in sorted((attempt/'quiet').glob('*/telemetry.jsonl')):
        samples = [json.loads(line) for line in path.read_text().splitlines()]
        record_path = path.with_name('run.json')
        record = json.loads(record_path.read_text()) if record_path.exists() else None
        result = replay_attempt(samples, record, version, backend)
        result.update(run=path.parent.name, original_valid=record['valid'] if record else None,
                      original_reasons=record['reasons'] if record else ['interrupted; no run.json'],
                      source={'path':str(path),'sha256':sha(path),
                              'run_sha256':sha(record_path) if record else None})
        result['sampled_phases_quiet'] = not any(result['phase_reasons'].values())
        (out/(path.parent.name+'.json')).write_text(json.dumps(result,indent=2))
        results.append(result)
        summary_path = attempt/'timed'/path.parent.name/'summary.json'
        if summary_path.exists():
            summaries[str(summary_path)] = json.loads(summary_path.read_text())
        if record:
            runs.append({**record, 'artifacts':{'summary':str(summary_path)}})
        print(f"{result['run']}: quiet={result['quiet_valid']}; complete={result['complete']}", flush=True)
    policy = timing_policy(version, backend, 'demo1-dense', 'coarse')
    metrics = batch_metrics({'same_binary_for_both_labels':True}, runs, summaries, version)
    (out/'replay.json').write_text(json.dumps({'policy':version, 'quiet_policy':policy,
        'timing_valid':False, 'valid':False, 'attempts':results, 'metrics':metrics,
        'lost_sensitivity':{key:value['lost_sensitivity'] for key,value in metrics.items()},
        'scope':'Descriptive replay of all retained timings; original rejections remain immutable and missing invocations are never imputed.'},indent=2))
    counts = collections.Counter((r['complete'],r['quiet_valid']) for r in results)
    print(dict(counts))
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--attempt',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--policy',choices=[VERSION,V4],default=VERSION)
    parser.add_argument('--backend',choices=['native','chrome'])
    args = parser.parse_args()
    replay_directory(args.attempt, args.out, args.policy, args.backend)
