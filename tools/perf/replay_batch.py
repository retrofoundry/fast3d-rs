#!/usr/bin/env python3
import argparse
import collections
import json
from pathlib import Path

from protocol import POLICY, replay_attempt, sha


def replay_directory(attempt, out):
    out.mkdir(parents=True, exist_ok=False)
    results = []
    for path in sorted((attempt/'quiet').glob('*/telemetry.jsonl')):
        samples = [json.loads(line) for line in path.read_text().splitlines()]
        record_path = path.with_name('run.json')
        record = json.loads(record_path.read_text()) if record_path.exists() else None
        result = replay_attempt(samples, record)
        result.update(run=path.parent.name, original_valid=record['valid'] if record else None,
                      original_reasons=record['reasons'] if record else ['interrupted; no run.json'],
                      source={'path':str(path),'sha256':sha(path),
                              'run_sha256':sha(record_path) if record else None})
        result['sampled_phases_quiet'] = not any(result['phase_reasons'].values())
        (out/(path.parent.name+'.json')).write_text(json.dumps(result,indent=2))
        results.append(result)
        print(f"{result['run']}: quiet={result['quiet_valid']}; complete={result['complete']}", flush=True)
    (out/'replay.json').write_text(json.dumps({'policy':POLICY,'attempts':results},indent=2))
    counts = collections.Counter((r['complete'],r['quiet_valid']) for r in results)
    print(dict(counts))
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--attempt',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    args = parser.parse_args()
    replay_directory(args.attempt, args.out)
