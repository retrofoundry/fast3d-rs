#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from types import SimpleNamespace

from protocol import sha, utc
from report import CAPTURES, batch_report, summarize


def run(args):
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    directory = Path(__file__).resolve().parent
    workloads = ['demo1-dense','demo2','source','demo1','pinned']
    cells = [f'{rev}.{wl}.{cfg}' for wl in workloads for cfg in ['coarse','counters']
             for rev in ['parent','candidate']]
    plan = {'attempts':[], 'cells':cells,
            'binaries':{rev:{'path':str(getattr(args,rev)), 'sha256':sha(getattr(args,rev))}
                        for rev in ['parent','candidate']}}
    with (out/'run.log').open('x', buffering=1) as log:
        subprocess.run([sys.executable,str(directory/'test_native.py')],
                       env={**os.environ,'B2_CANDIDATE_BIN':str(args.candidate),'B2_PARENT_BIN':str(args.parent)},
                       stdout=log,stderr=log,check=True)
        log.write(f'{utc()} hardware regression passed; settling {args.settle}s\n')
        time.sleep(args.settle)
        for workload in workloads:
            expected = 600 if workload=='source' else CAPTURES[workload]['observed'][1]-CAPTURES[workload]['observed'][0]+1
            for configuration in ['coarse','counters']:
                for pair in range(1,6):
                    order = ['parent','candidate'] if pair%2 else ['candidate','parent']
                    for revision in order:
                        name = f'{workload}-{configuration}-{pair}-{revision}'
                        timed = out/'timed'/name
                        quiet = out/'quiet'/name
                        command = [str(getattr(args,revision)),
                                   'source' if workload=='source' else 'sequence',
                                   str(args.scene if workload=='source' else args.captures/(workload+'.f3dcap')),
                                   str(timed),configuration]
                        if revision=='candidate' and workload!='source':
                            command.append('--parent-compatible')
                        log.write(f'{utc()} {name}\n')
                        subprocess.run([sys.executable,str(directory/'protocol.py'),'--out',str(quiet),
                                        '--attestation',args.attestation,'--scheduled-start',args.scheduled_start,
                                        '--scheduled-end',args.scheduled_end,'--pair',f'{workload}-{configuration}-{pair}',
                                        '--revision',revision,'--workload',workload,'--configuration',configuration,
                                        '--allowed-idle','WindowServer','--']+command, stdout=log,stderr=log)
                        plan['attempts'].append({'quiet_run':str(quiet/'run.json'),
                                                 'summary':str(timed/'summary.json'),'expected_observed':expected})
                        (out/'attempts.json').write_text(json.dumps(plan,indent=2))
        for entry in plan['attempts']:
            summary = Path(entry['summary'])
            frames = summary.with_name('frames.jsonl')
            if frames.exists():
                record = json.loads(Path(entry['quiet_run']).read_text())
                try:
                    summarize(SimpleNamespace(input=frames,out=summary,
                                              configuration=record['configuration'],
                                              expected_observed=entry['expected_observed']))
                except ValueError as error:
                    log.write(f'{frames}: {error}\n')
        batch_report(SimpleNamespace(input=out/'attempts.json',out=out/'batch.json'))
        valid = json.loads((out/'batch.json').read_text())['valid']
        log.write(f'{utc()} batch valid={valid}\n')
    return 0 if valid else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ['out','candidate','parent','captures','scene']:
        parser.add_argument('--'+name,type=Path,required=True)
    for name in ['attestation','scheduled-start','scheduled-end']:
        parser.add_argument('--'+name,required=True)
    parser.add_argument('--settle',type=float,default=90)
    raise SystemExit(run(parser.parse_args()))
