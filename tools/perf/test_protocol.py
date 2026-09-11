import copy
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from protocol import POLICY, VERSION, sample_reasons, spread, invalidate_pairs, disallowed_process, resting_profile
from report import CAPTURES, validate_manifest, validate_pair_order, break_even, cost_report, batch_report

class ProtocolTests(unittest.TestCase):
    def test_hash_reports_separate_algorithms_and_unused_representations(self):
        def cost(hash_version, representation, k):
            return {'hash_version':hash_version,'class':f'{representation}.0-2.tlut0.32x32',
                    'set':'rotating','costs_ns':{'d':100,'k':k,'h':10,'m':20},
                    'h_min':(k+20)/110,'perfect_hit_viable':100>k+10,'uncertainty_ns':1,
                    'baseline_model_residual_ns':0,'hit_model_residual_ns':0,'miss_model_residual_ns':0,
                    'encoded_bytes':2048,'output_extent':[32,32],'output_bytes':4096,
                    'components_ns':{'baseline_total':[100]*5,'candidate_hit_total':[k+10]*5,
                                     'candidate_miss_total':[k+120]*5,'owned_copy':[1]*5}}
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            costs=root/'costs.jsonl'; hits=root/'hits.jsonl'; out=root/'report.json'
            data=[cost(h,r,k if r=='Tile' else 1) for h,k in [('fnv1a64-v1',200),('xxh3-64-v1',20)] for r in ['Tile','Lookup']]
            costs.write_text(''.join(json.dumps(r)+'\n' for r in data))
            hits.write_text(json.dumps({'observed':True,'classes':{'Tile.0-2.tlut0.32x32.texture0':{'requests':10,'hits':10,'rejected':0}}})+'\n')
            break_even(SimpleNamespace(costs=costs,hits=hits,out=out))
            report=json.loads(out.read_text())['by_hash']
            for h,saving in [('fnv1a64-v1',-110),('xxh3-64-v1',70)]:
                window=report[h]['observed']
                self.assertEqual(len(window['classes']),1)
                self.assertEqual(window['classes'][0]['predicted_saving_ns_per_request_upper_bound'],saving)
                self.assertAlmostEqual(window['request_weighted_predicted_saved_ms_upper_bound'],saving*10/1e6)
                self.assertEqual(window['unobserved_cost_classes'][0]['observed_requests'],0)
            cost_report(SimpleNamespace(input=costs,out=out))
            report=json.loads(out.read_text())['by_hash']
            self.assertEqual(len(report),2)
            self.assertFalse(report['fnv1a64-v1']['Tile.0-2.tlut0']['viable_sizes'])
            self.assertTrue(report['xxh3-64-v1']['Tile.0-2.tlut0']['viable_sizes'])
            attempts=[]
            for i in range(5):
                run=root/f'run-{i}.json'
                run.write_text(json.dumps({'quiet_protocol_version':VERSION,'valid':True,'reasons':[],'pair':i,'revision':'candidate',
                                          'workload':'authored','configuration':'preflight','actual_utc_interval':[i,i+1]}))
                attempts.append({'quiet_run':str(run),'costs':str(costs)})
            plan=root/'plan.json'; plan.write_text(json.dumps({'comparison':False,'attempts':attempts}))
            batch_report(SimpleNamespace(input=plan,out=out))
            batch=json.loads(out.read_text())
            self.assertTrue(batch['valid'])
            self.assertEqual(len(batch['spreads']),16)

    def samples(self,n=30):
        return [{'monotonic':float(i),'idle_percent':99,'idle_age_seconds':.1,'background_cores':.01,'swapouts':0,'thermal_throttled':False,'disallowed':[],'power':'AC','power_mode':'fixed','processes':[]} for i in range(n)]
    def test_benchmark_run_admission_rejects_contention_and_spread(self):
        profile = resting_profile(self.samples())
        self.assertEqual(sample_reasons(self.samples(),'preflight',profile),[])
        chrome={'command':'/Applications/Google Chrome.app/Google Chrome','cores':0.0}
        self.assertFalse(disallowed_process(chrome,[]))
        self.assertFalse(disallowed_process(chrome,['Google Chrome']))
        chrome['cores']=.04
        self.assertTrue(disallowed_process(chrome,['Google Chrome']))
        for field,value,phase in [('thermal_throttled',True,'run'),('processes',[{'pid':12,'command':'cargo','cores':0,'exemption':None}],'run'),('swapouts',1,'run'),('idle_age_seconds',2,'run'),('power','battery','run')]:
            samples=self.samples(); samples[10][field]=value
            self.assertTrue(sample_reasons(samples,phase,profile),field)
        samples=self.samples()
        for sample in samples: sample['idle_percent']=94
        self.assertTrue(sample_reasons(samples,'preflight',profile))
        samples=self.samples(); samples[10]['monotonic']+=.7
        self.assertIn('monitor gap',sample_reasons(samples,'run',profile))
        samples=self.samples(); samples[10]['background_cores']=.26; samples[11]['background_cores']=.26
        self.assertIn('background above resting ceiling 0.0600 core twice',sample_reasons(samples,'run',profile))
        samples=self.samples()
        for row in samples: row['background_cores']=.11
        self.assertIn('background average above resting allowance 0.0600 core',sample_reasons(samples,'run',profile))
        self.assertTrue(spread([97.5,100,100,100,102.5])['valid'])
        self.assertFalse(spread([97.49,100,100,100,102.5])['valid'])
        self.assertFalse(spread([100]*4)['valid'])
        pair=invalidate_pairs([{'pair':1,'valid':True,'reasons':[]},{'pair':1,'valid':False,'reasons':['contention']}])
        self.assertFalse(pair[0]['valid']); self.assertEqual(len(pair),2)
        def member(pair, revision):
            return {'pair':pair,'revision':revision,'workload':'demo2','configuration':'coarse','valid':True}
        good=[member(1,'parent'),member(1,'candidate'),member(2,'candidate'),member(2,'parent')]
        self.assertEqual(validate_pair_order(good),[])
        self.assertTrue(validate_pair_order(good[:1]))
        self.assertTrue(validate_pair_order(good[:2]+[member(2,'parent'),member(2,'candidate')]))
    def test_benchmark_manifest_requires_provenance_and_quiet_evidence(self):
        manifest={'captures':{name:{**item,'prefix_start':1} for name,item in CAPTURES.items()},'source':{'source_sha256':'a','rgba8_sha256':'b','rgba16_sha256':'c','generator':'env-xor-v1','synthetic':True},'revisions':{'test':1},'build':{'test':1},'host':{'test':1},'clock':{'test':1},'preflight':{'cost_paths':['smoke-costs.jsonl']}}
        self.assertEqual(validate_manifest(manifest,False),[])
        self.assertTrue(validate_manifest(manifest))
        missing=copy.deepcopy(manifest); missing['preflight']['cost_paths']=[]
        self.assertIn('missing break-even cost inputs',validate_manifest(missing,False))
        for field in ['demo2','demo1-dense']:
            missing=copy.deepcopy(manifest); del missing['captures'][field]
            self.assertTrue(validate_manifest(missing,False))
        missing=copy.deepcopy(manifest); missing['source']['synthetic']=False
        self.assertTrue(validate_manifest(missing,False))
        missing=copy.deepcopy(manifest); missing['captures']['demo2']['prefix_start']=2600
        self.assertTrue(validate_manifest(missing,False))
        missing=copy.deepcopy(manifest); missing['quiet_protocol_version']=VERSION; missing['quiet_policy']=POLICY; missing['runs']=[{'valid':True}]
        self.assertIn('run missing reasons',validate_manifest(missing))

if __name__=='__main__': unittest.main()
