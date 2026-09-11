#!/usr/bin/env python3
import argparse
import collections
import json
import statistics
import zipfile
from pathlib import Path
from protocol import POLICY, VERSION, command, invalidate_pairs, sha, spread

CAPTURES = {
    'demo1-dense': {'frames':1519,'warmup':1399,'observed':[1400,1519],'route':'demo1','sha256':'2467ff249136182d2ad6a837f17f9dad4b405dfe45af10a5c8956dd3685328fa'},
    'demo1': {'frames':1600,'warmup':1199,'observed':[1200,1600],'route':'demo1-control','sha256':'bec5477ac49dbc2d37fe5d20063e4795aecb63558ac63a865bc09c11c237643b'},
    'pinned': {'frames':1419,'warmup':1399,'observed':[1400,1419],'route':'demo1-control','sha256':'80d169162416b01385184fc49ea6a6cade17e5fe51045ec02b73796fb75b8572'},
    'demo2': {'frames':2719,'warmup':2599,'observed':[2600,2719],'route':'demo2','sha256':'bec7c92cd3c87b8a25367d5439e572d7851009460309263560a302f5bc5c9c09','bytes':573989192,'provenance':'second-route-provenance.md','scene':'stone masonry, grass, dirt slope and sky; second attract route of the same build'},
}

def rows(path):
    with Path(path).open() as stream:
        for line in stream:
            yield json.loads(line)

def validate_manifest(manifest, require_quiet=True):
    reasons=[]
    for name, expected in CAPTURES.items():
        item=manifest.get('captures',{}).get(name,{})
        for field in ['sha256','frames','warmup','observed','route']:
            if item.get(field)!=expected[field]:
                reasons.append(f'{name}: missing or changed {field}')
        if item.get('prefix_start')!=1:
            reasons.append(f'{name}: missing reset prefix')
    texture=manifest.get('source',{})
    for field in ['source_sha256','rgba8_sha256','rgba16_sha256','generator']:
        if not texture.get(field):
            reasons.append(f'source: missing {field}')
    if texture.get('synthetic') is not True:
        reasons.append('substitute texture must be labeled synthetic')
    for field in ['revisions','build','host','clock','preflight']:
        if not manifest.get(field):
            reasons.append(f'missing {field}')
    if not manifest.get('preflight',{}).get('cost_paths'):
        reasons.append('missing break-even cost inputs')
    if require_quiet:
        if any(isinstance(v,dict) and 'unavailable' in v for v in manifest.get('host',{}).values()):
            reasons.append('host probes unavailable')
        if manifest.get('quiet_protocol_version')!=VERSION or manifest.get('quiet_policy')!=POLICY:
            reasons.append('incomplete quiet policy')
        if not manifest.get('batch_valid'):
            reasons.append('missing or rejected complete paired batch')
        runs=manifest.get('runs',[])
        if not runs:
            reasons.append('missing quiet runs')
        for run in runs:
            for field in ['scheduled_utc_interval','actual_utc_interval','operator_attestation','monitor_commands','telemetry','pair','revision','reasons']:
                if field not in run or (field not in ['reasons','telemetry'] and not run[field]):
                    reasons.append(f'run missing {field}')
            if run.get('valid') and not run.get('telemetry'):
                reasons.append('valid run missing telemetry')
            if not run.get('valid') and not run.get('reasons'):
                reasons.append('invalid run missing reason')
            for evidence in run.get('telemetry',[]):
                path=Path(evidence['path'])
                if not path.exists() or sha(path)!=evidence['sha256']:
                    reasons.append('missing or changed telemetry')
        if not manifest.get('spreads') or any(not s['valid'] for s in manifest['spreads'].values()):
            reasons.append('missing or unstable five-run spreads')
    return sorted(set(reasons))

def host_probe(args):
    try:
        return {'command':args,'output':command(args)}
    except RuntimeError as error:
        return {'command':args,'unavailable':str(error)}


def manifest(args):
    out=args.out
    out.mkdir(parents=True,exist_ok=True)
    root=Path(__file__).resolve().parents[2]
    build=args.build
    compiler=command(['rustc','-Vv'])
    native=next(line.split(': ',1)[1] for line in compiler.splitlines() if line.startswith('host: '))
    graph=json.dumps({target:json.loads(command(['cargo','metadata','--offline','--format-version','1','--filter-platform',target,'--manifest-path',str(build/'Cargo.toml')])) for target in [native,'wasm32-unknown-unknown']},indent=2)
    (out/'dependencies.json').write_text(graph)
    (out/'Cargo.lock').write_bytes((build/'Cargo.lock').read_bytes())
    diff=command(['git','diff','HEAD','--'],root)
    (out/'candidate.diff').write_text(diff)
    untracked=command(['git','ls-files','--others','--exclude-standard'],root)
    (out/'untracked.txt').write_text(untracked)
    changed=command(['git','diff','HEAD','--name-only'],root).splitlines()
    with zipfile.ZipFile(out/'candidate-source.zip','w',compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(set(changed+untracked.splitlines())):
            path=root/name
            if path.is_file() and '__pycache__' not in name:
                archive.write(path,name)
    source=json.loads((args.source_run/'input.json').read_text())
    captures={name:{**data,'path':str(args.captures/(name+'.f3dcap')),'prefix_start':1} for name,data in CAPTURES.items()}
    for name, item in captures.items():
        input_record=args.evidence/name/'input.json'
        item['cpu_admission']=str(input_record) if input_record.exists() else None
        admission=args.evidence/name/'admission.json'
        item['all_frame_admission']=json.loads(admission.read_text()) if admission.exists() else None
    runs=[]
    for run in args.runs:
        runs.append(json.loads(run.read_text()))
    invalidate_pairs(runs)
    value={'stage':'B1 smoke; B2 quiet measurements pending','captures':captures,'source':source,
           'revisions':{'fast3d':command(['git','rev-parse','HEAD'],root).strip(),'base':'509cf9b5abdfc76a64532cf87d78f20363fe254b','assembler':command(['git','rev-parse','HEAD'],args.assembler).strip(),'assembler_pin':'f19a9c34c763f64aace0b61c26b65dcb490415e5','sm64':'4a9dcf0d0a82a637b19b401f969639c9f4e0c83a','rt64':'43373749dac9bbc1b653e6a02aed40a9e1783bed'},
           'build':{'profile':'release, debug=1','features':['capture','profiling'],'rustc':compiler,'cargo':command(['cargo','-V']),'dependencies':str(out/'dependencies.json'),'dependency_graph_sha256':sha(out/'dependencies.json'),'lock_sha256':sha(out/'Cargo.lock'),'local_diff_sha256':sha(out/'candidate.diff'),'untracked':str(out/'untracked.txt'),'candidate_source_archive':str(out/'candidate-source.zip'),'candidate_source_sha256':sha(out/'candidate-source.zip')},
           'host':{'os':host_probe(['sw_vers']),'uname':host_probe(['uname','-a']),'cpu':host_probe(['sysctl','-n','machdep.cpu.brand_string']),'logical_cpus':host_probe(['sysctl','-n','hw.logicalcpu'])},
           'clock':{'implementation':'Instant native; performance.now browser','calibration':'input/setup clock arrays are [resolution_ms, event_ms]; cost rows also name both fields in ns','units':'CPU-side elapsed ms; not GPU or OS thread CPU time'},
           'preflight':{'budget_bytes':16*1024*1024,'policy':'LRU, no admission filter, canonical equality on digest match, two-frame conservative pins','hashes':sorted({r['hash_version'] for p in args.costs for r in rows(p)}),'trace_hash':'fnv1a64-v1','identity':'fast3d-b1-bank-v1, little-endian metadata + 4096 bank bytes + reconstructed linear bytes','trace_live_reference_status':'upper bound: scene ownership not traced; pinning and bypass accounting cannot establish a production hit rate','cost_paths':[str(p) for p in args.costs]},
           'quiet_protocol_version':VERSION,'quiet_policy':POLICY,'runs':runs,'pair_order':[{'pair':r.get('pair'),'revision':r.get('revision')} for r in runs],'spreads':{},
           'hardware_validation':'pending native Metal, Windows DX12/static-DXC, Chrome WebGPU, pinned IMAGE oracle',
           'second_route_provenance':'/Volumes/DS Vault/hub/scratch/fast3d/second-route-provenance.md'}
    if args.batch:
        batch=json.loads(args.batch.read_text())
        value['runs']=batch['attempts']
        value['spreads']=batch['spreads']
        value['pair_order']=batch['pair_order']
        value['batch_valid']=batch['valid']
        value['pair_rejections']=batch['pair_rejections']
    value['artifacts']=[{'directory':str(directory),'files':[{'path':str(path),'sha256':sha(path)} for path in directory.iterdir() if path.name in ['input.json','setup.json','driver.json','summary.json','admission.json','cases.json','hash-inputs.json','costs.jsonl','break-even.json','browser-capabilities.json','frames.jsonl','requests.jsonl','hits.jsonl','result.json','result.jsonl','env.rgba8','env.rgba16']]} for directory in args.artifacts]
    binary=build/'target/release/b1-perf'
    value['build']['native_binary']=str(binary)
    value['build']['native_binary_sha256']=sha(binary) if binary.exists() else None
    verification=args.evidence/'trace-regeneration.json'
    if verification.exists():
        value['trace_regeneration']={'path':str(verification),'sha256':sha(verification)}
    value['quiet_evidence_rejections']=validate_manifest(value)
    (out/'manifest.json').write_text(json.dumps(value,indent=2))
    print(out/'manifest.json')

def summarize(args):
    frames=list(rows(args.input))
    groups={'cold':[f for f in frames if not f.get('observed')], 'observed':[f for f in frames if f.get('observed')]}
    if any(f['serial']==1400 for f in frames):
        groups['serial-1400-after-prefix']=[f for f in frames if f['serial']==1400]
    output={}
    for name, selected in groups.items():
        counts=collections.Counter()
        decodes=collections.defaultdict(collections.Counter)
        timing=collections.Counter()
        values=[]
        peaks={}
        for frame in selected:
            profile=frame.get('profile',{})
            counts.update(profile.get('counters',{}))
            for key,value in profile.get('gauges',{}).items():
                peaks[key]=max(peaks.get(key,0),value)
            for key,row in profile.get('decodes',{}).items():
                decodes[key].update(row)
            for key,row in profile.get('timings',{}).items():
                timing[key+'.inclusive_ms']+=row['inclusive_ms']
                timing[key+'.exclusive_ms']+=row['exclusive_ms']
            total=sum(profile.get('timings',{}).get(phase,{}).get('inclusive_ms',0) for phase in ['process_dl','begin_frame','presentation'])
            values.append((total,frame['serial']))
        timed=any(any(phase in f.get('profile',{}).get('timings',{}) for phase in ['process_dl','begin_frame','presentation']) for f in selected)
        ordered=sorted(v for v,_ in values) if timed else []
        output[name]={'frames':len(selected),'counters':counts,'memory_high_water_bytes':peaks,'decodes':dict(decodes),'timings':timing,'cpu_ms':sum(ordered) if timed else None,'cpu_ms_per_frame':sum(ordered)/len(ordered) if ordered else None,'median_ms':statistics.median(ordered) if ordered else None,'p95_ms':ordered[min(len(ordered)-1,int(len(ordered)*.95))] if ordered else None,'slowest':sorted(values,reverse=True)[:10] if timed else [],'library_timing_available':timed,
                      'assembly_ms':sum(f.get('assembly_ms',0) for f in selected),'wait_ms':sum(f.get('wait_ms',0) for f in selected),'adapter_ms':sum(f.get('adapter_ms',0) for f in selected),'readback_ms':sum(f.get('readback_ms',0) for f in selected)}
    args.out.write_text(json.dumps(output,indent=2))

def cost_prediction(cost, h):
    c=cost['costs_ns']
    saving=c['d']-(c['k']+h*c['h']+(1-h)*(c['d']+c['m']))
    components=cost['components_ns']
    baseline=statistics.median(components['baseline_total'])
    hit=statistics.median(components['candidate_hit_total'])
    miss=statistics.median(components['candidate_miss_total'])
    residuals={k:cost.get(k) for k in ['baseline_model_residual_ns','hit_model_residual_ns','miss_model_residual_ns']}
    reconciles=all(v is not None and abs(v)<=cost['uncertainty_ns'] for v in residuals.values())
    return {'hash_version':cost['hash_version'],'h_min':cost['h_min'],'perfect_hit_viable':cost['perfect_hit_viable'],
            'predicted_saving_ns_per_request_upper_bound':saving,'uncertainty_ns':cost['uncertainty_ns'],
            'boundary_resolved':reconciles and abs(saving)>cost['uncertainty_ns'], 'model_reconciles':reconciles,
            **residuals,'encoded_bytes':cost['encoded_bytes'],'extent':cost['output_extent'],'output_bytes':cost['output_bytes'],
            'end_to_end_saved_ns_per_request_upper_bound':baseline-h*hit-(1-h)*miss,
            'end_to_end_h_min':(miss-baseline)/(miss-hit) if miss>hit else None,
            'end_to_end_perfect_hit_viable':baseline>hit}


def break_even(args):
    costs=collections.defaultdict(dict)
    for row in rows(args.costs):
        if row['set']=='rotating':
            if row['class'] in costs[row['hash_version']]:
                raise ValueError('duplicate hash/class cost row')
            costs[row['hash_version']][row['class']]=row
    totals={False:collections.defaultdict(collections.Counter),True:collections.defaultdict(collections.Counter)}
    for frame in rows(args.hits):
        for role,row in frame['classes'].items():
            key=role.rsplit('.',1)[0]
            totals[frame['observed']][key].update(row)
    report={}
    for hash_version, hash_costs in costs.items():
        windows={}
        for observed,classes in totals.items():
            output=[]
            for key,counts in classes.items():
                cost=hash_costs.get(key)
                if cost is None:
                    output.append({'class':key,'hash_version':hash_version,'status':'missing cost class','counts':counts})
                    continue
                n=counts['requests']-counts['rejected']
                h=counts['hits']/n if n else 0
                prediction=cost_prediction(cost,h)
                output.append({'class':key,'counts':counts,'hit_fraction_upper_bound':h,**prediction,
                               'predicted_saved_ms_upper_bound':prediction['predicted_saving_ns_per_request_upper_bound']*n/1e6,
                               'end_to_end_saved_ms_upper_bound':prediction['end_to_end_saved_ns_per_request_upper_bound']*n/1e6})
            unobserved=[{'class':c['class'],'observed_requests':0,**cost_prediction(c,1.0)} for key,c in hash_costs.items() if key not in classes]
            windows['observed' if observed else 'cold']={
                'classes':output,'unobserved_cost_classes':unobserved,
                'request_weighted_predicted_saved_ms_upper_bound':sum(r.get('predicted_saved_ms_upper_bound',0) for r in output),
                'request_weighted_end_to_end_saved_ms_upper_bound':sum(r.get('end_to_end_saved_ms_upper_bound',0) for r in output)}
        report[hash_version]=windows
    args.out.write_text(json.dumps({'by_hash':report,
        'hit_trace':str(args.hits),'cost_input':str(args.costs),
        'boundary_policy':'Per hash, workload window and class. Zero-request classes are excluded from forecasts. Enumerated sizes only; preliminary smoke, no activation decision.'},indent=2))


def validate_pair_order(runs):
    pairs=collections.defaultdict(list)
    reasons=[]
    for run in runs:
        pairs[run['pair']].append(run)
    previous={}
    for pair,members in pairs.items():
        if not all(m['valid'] for m in members):
            continue
        if len(members)!=2 or {m['revision'] for m in members}!={'parent','candidate'}:
            reasons.append(f'{pair}: requires parent and candidate')
            continue
        configurations={(m['workload'],m['configuration']) for m in members}
        if len(configurations)!=1:
            reasons.append(f'{pair}: mismatched workload/configuration')
            continue
        config=configurations.pop()
        order=[m['revision'] for m in members]
        if previous.get(config)==order:
            reasons.append(f'{pair}: order did not alternate')
        previous[config]=order
    return reasons


def cost_report(args):
    groups=collections.defaultdict(lambda:collections.defaultdict(list))
    for row in rows(args.input):
        if row['set']!='rotating':
            continue
        group='.'.join(row['class'].split('.')[:3])
        c=row['costs_ns']
        groups[row['hash_version']][group].append({'class':row['class'],**cost_prediction(row,1.0),
            'decode_only_viable':c['d']>c['k']+c['h'],'copy_control_ns':statistics.median(row['components_ns']['owned_copy'])})
    output={}
    for hash_version,hash_groups in groups.items():
        output[hash_version]={}
        for group,values in hash_groups.items():
            viable=[r for r in values if r['perfect_hit_viable']]
            output[hash_version][group]={'smallest_measured_viable_encoded_bytes':min((r['encoded_bytes'] for r in viable),default=None),'measured_sizes':values,'viable_sizes':viable}
    args.out.write_text(json.dumps({'by_hash':output,'copy_saving_in_d':0,
        'decode_only_and_full_candidate':'identical: no copy saving credited to cache hits',
        'boundary_policy':'Authored classes have no workload frequency. Keep representations and hashes separate; no monotonic cutoff or activation decision.'},indent=2))


def batch_report(args):
    plan=json.loads(args.input.read_text())
    runs=[]
    totals=collections.defaultdict(list)
    for entry in plan['attempts']:
        run=json.loads(Path(entry['quiet_run']).read_text())
        run['artifacts']=entry
        runs.append(run)
    runs.sort(key=lambda run: run['actual_utc_interval'][0])
    invalidate_pairs(runs)
    comparison=plan.get('comparison',True)
    pair_reasons=validate_pair_order(runs) if comparison else []
    if not comparison and any('costs' not in r['artifacts'] for r in runs):
        pair_reasons.append('unpaired batches are only supported for cache-cost components')
    for run in runs:
        if not run['valid']:
            continue
        entry=run['artifacts']
        prefix=f"{run['revision']}.{run['workload']}.{run['configuration']}"
        if 'summary' in entry:
            summary=json.loads(Path(entry['summary']).read_text())
            for window in ['cold','observed']:
                for metric in ['cpu_ms','assembly_ms']:
                    if summary[window][metric] is not None and summary[window][metric]>0:
                        totals[f'{prefix}.{window}.{metric}'].append(summary[window][metric])
                for phase,value in summary[window]['timings'].items():
                    if value>0:
                        totals[f'{prefix}.{window}.{phase}'].append(value)
        if 'costs' in entry:
            for row in rows(entry['costs']):
                for component,values in row['components_ns'].items():
                    totals[f"{prefix}.{row['hash_version']}.{row['class']}.{row['set']}.{component}"].append(statistics.median(values))
    result={'quiet_protocol_version':VERSION,'attempts':runs,'pair_order':[{'pair':r['pair'],'revision':r['revision']} for r in runs],'spreads':{key:spread(values) for key,values in totals.items()},'spread_limit':.05,'comparison':comparison}
    result['pair_rejections']=pair_reasons
    result['valid']=bool(totals) and not pair_reasons and all(value['valid'] for value in result['spreads'].values())
    args.out.write_text(json.dumps(result,indent=2))


def compare(args):
    left=list(rows(args.input)); right=list(rows(args.other))
    errors=[]
    if len(left)!=len(right):
        errors.append('frame count differs')
    for a,b in zip(left,right):
        for field in ['serial','rgba8_sha256','summaries','summary','diagnostics','tasks','width','height']:
            if field in a or field in b:
                if a.get(field)!=b.get(field):
                    errors.append(f'serial {a["serial"]}: {field} differs')
        if not a.get('rgba8_sha256') or not b.get('rgba8_sha256'):
            errors.append(f'serial {a["serial"]}: missing pixel readback')
    args.out.write_text(json.dumps({'frames':len(left),'valid':not errors,'errors':errors},indent=2))
    if errors:
        raise SystemExit(1)

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    subs=parser.add_subparsers(dest='action',required=True)
    p=subs.add_parser('manifest')
    for name in ['build','assembler','captures','source-run','evidence','out']:
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--costs',type=Path,nargs='*',default=[])
    p.add_argument('--runs',type=Path,nargs='*',default=[])
    p.add_argument('--batch',type=Path)
    p.add_argument('--artifacts',type=Path,nargs='*',default=[])
    for name in ['summarize','break-even','compare','batch','cost-report']:
        p=subs.add_parser(name)
        if name=='break-even':
            p.add_argument('--costs',type=Path,required=True); p.add_argument('--hits',type=Path,required=True)
        else:
            p.add_argument('--input',type=Path,required=True)
        if name=='compare':
            p.add_argument('--other',type=Path,required=True)
        p.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    {'manifest':manifest,'summarize':summarize,'break-even':break_even,'compare':compare,'batch':batch_report,'cost-report':cost_report}[args.action](args)
