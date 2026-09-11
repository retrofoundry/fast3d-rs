#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

parser=argparse.ArgumentParser()
for name in ['build','scene','out','chromedriver']:
    parser.add_argument('--'+name,type=Path,required=True)
parser.add_argument('--webdriver',type=Path,default=Path(__file__).resolve().parents[2]/'fast3d/webdriver.json')
parser.add_argument('--mode',choices=['counters','coarse','detailed','trace'],default='coarse')
parser.add_argument('--kind',choices=['source','preflight','sequence'],default='source')
parser.add_argument('--readback',action='store_true')
parser.add_argument('--one-frame',action='store_true')
parser.add_argument('--cases',type=Path)
parser.add_argument('--capture',type=Path)
parser.add_argument('--port',type=int,default=8765)
parser.add_argument('--driver-port',type=int,default=9515)
args=parser.parse_args()
args.out.mkdir(parents=True,exist_ok=True)
base=f'http://127.0.0.1:{args.driver_port}'
def request(method,path,value=None):
    data=None if value is None else json.dumps(value).encode()
    req=urllib.request.Request(base+path,data=data,method=method,headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(req,timeout=3600) as response:
        result=json.load(response)
    if isinstance(result.get('value'),dict) and 'error' in result['value']:
        raise RuntimeError(result)
    return result.get('value')

serve=[sys.executable,str(Path(__file__).with_name('serve.py')),'--build',str(args.build),'--scene',str(args.scene),'--out',str(args.out),'--port',str(args.port)]
for name in ['cases','capture']:
    if getattr(args,name):
        serve += ['--'+name,str(getattr(args,name))]
with (args.out/'browser-processes.log').open('w') as logs:
    server=subprocess.Popen(serve,stdout=logs,stderr=logs)
    driver=subprocess.Popen([str(args.chromedriver),'--port='+str(args.driver_port)],stdout=logs,stderr=logs)
    session=None
    try:
        for attempt in range(100):
            try:
                request('GET','/status')
                break
            except (urllib.error.URLError,ConnectionError):
                time.sleep(.1)
        options=json.loads(args.webdriver.read_text())
        result=request('POST','/session',{'capabilities':{'alwaysMatch':{'browserName':'chrome',**options}}})
        session=result['sessionId']
        (args.out/'browser-capabilities.json').write_text(json.dumps(result,indent=2))
        prefix='/session/'+session
        request('POST',prefix+'/timeouts',{'script':3600000})
        request('POST',prefix+'/url',{'url':f'http://127.0.0.1:{args.port}/'})
        script='''
const done = arguments[arguments.length-1];
(async () => {
    const module = await import('/pkg/b1_perf.js');
    await module.default();
    const options = arguments[0];
    const started = new Date().toISOString();
    let data;
    if (options.kind === 'source') data = await module.source(await (await fetch('/scene.n64')).text(),options.mode,options.readback,options.one_frame);
    else if (options.kind === 'sequence') data = await module.sequence(new Uint8Array(await (await fetch('/capture.f3dcap')).arrayBuffer()),options.mode,options.readback,options.one_frame);
    else data = module.preflight(options.cases ? await (await fetch('/cases.json')).text() : '');
    const result = JSON.parse(data);
    result.browser = navigator.userAgent;
    result.started_utc = started;
    result.ended_utc = new Date().toISOString();
    result.isolated = crossOriginIsolated;
    const response = await fetch('/artifact/result.json',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(result)});
    if (!response.ok) throw new Error(await response.text());
    done({path:await response.text()});
})().catch(error => done({failure:String(error)}));
'''
        result=request('POST',prefix+'/execute/async',{'script':script,'args':[{'kind':args.kind,'mode':args.mode,'readback':args.readback,'one_frame':args.one_frame,'cases':bool(args.cases)}]})
        if 'failure' in result:
            raise RuntimeError(result['failure'])
        print(result['path'])
    finally:
        if session:
            request('DELETE','/session/'+session)
        driver.terminate(); server.terminate()
        driver.wait(); server.wait()
