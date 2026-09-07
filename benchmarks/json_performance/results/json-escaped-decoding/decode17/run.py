#!/usr/bin/env python3
"""Serial, fresh-process JSON benchmark for macOS; raw results are append-only.
Run on the benchmark host after staging .work/worker and .work/fixtures.
"""
import argparse, hashlib, json, math, os, random, re, signal, statistics, subprocess, time
from pathlib import Path
ROOT=Path(__file__).resolve().parent
P=argparse.ArgumentParser()
P.add_argument('--phase',choices=['verify','timing','memory','tape','all'],default='all')
P.add_argument('--repeat',type=int,default=3)
P.add_argument('--filter',default='')
P.add_argument('--baseline-worker',type=Path,help='add a second, freshly built Perry runtime arm')
P.add_argument('--worker',type=Path,default=ROOT/'.work/worker')
P.add_argument('--results-dir',type=Path,default=ROOT/'results')
P.add_argument('--operations',default='',help='comma-separated operation selection')
A=P.parse_args()
ENGINES={'perry':[str(A.worker.resolve())], 'node':['/Users/perry/nodebin/node',str(ROOT/'worker.js')], 'bun':['/Users/perry/.bun/bin/bun',str(ROOT/'worker.js')]}
if A.baseline_worker: ENGINES['baseline']=[str(A.baseline_worker.resolve())]
FIXTURES=json.loads((ROOT/'results/fixtures.json').read_text())
if A.filter: FIXTURES=[f for f in FIXTURES if A.filter in f['name']]
if A.operations:
    operations=set(A.operations.split(','))
    FIXTURES=[dict(f,operations=[op for op in f['operations'] if op in operations]) for f in FIXTURES]
OUT=A.results_dir
OUT.mkdir(parents=True,exist_ok=True)

def one(engine,f,op,n,warm=0,verify=False,tape=None,timeout=60):
    cmd=ENGINES[engine]+[str(ROOT/'.work/fixtures'/(f['name']+'.json')),op,str(n),str(warm)]
    if verify: cmd+=['verify']
    env=os.environ.copy()
    # Ambient experimental knobs must never silently change a benchmark.
    for key in list(env):
        if key.startswith('PERRY_'): del env[key]
    if tape is not None: env['PERRY_JSON_TAPE']=str(tape)
    started=time.time()
    p=subprocess.Popen(['/usr/bin/time','-l']+cmd,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,start_new_session=True)
    try:
        stdout,stderr=p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGKILL)
        p.communicate()
        return dict(engine=engine,fixture=f['name'],operation=op,iterations=n,warmup=warm,tape=tape,error='timeout',timeout_s=timeout)
    text=stdout.decode('utf8',errors='replace')
    err=stderr.decode('utf8',errors='replace')
    result=dict(engine=engine,fixture=f['name'],operation=op,iterations=n,warmup=warm,tape=tape,exit_code=p.returncode,process_wall_s=time.time()-started)
    match=re.search(r'^RESULT (.+)$',text,re.M)
    if p.returncode!=0 or not match:
        result.update(error='process failed',stderr=err[-5000:],stdout=text[:500]);return result
    v=match.group(1).split()
    if len(v)!=7: raise ValueError(v)
    result.update(dict(zip(['wall_ms','user_us','system_us','rss_before','rss_after','checksum','retained'],map(float,v))))
    if not all(math.isfinite(result[k]) for k in ['wall_ms','user_us','system_us']):
        result['error']='nonfinite measurement'
    result['cpu_ms']=(result['user_us']+result['system_us'])/1000
    for pattern,key in [(r'(\d+)\s+maximum resident set size','peak_rss'),(r'(\d+)\s+instructions retired','instructions'),(r'(\d+)\s+cycles elapsed','cycles'),(r'(\d+)\s+peak memory footprint','peak_footprint')]:
        m=re.search(pattern,err)
        if m: result[key]=int(m.group(1))
    m=re.search(r'^VERIFY (.*)$',text,re.M)
    if m:
        # JSON text can contain no literal newline except in pretty output.
        blob=text.split('VERIFY ',1)[1].rsplit('\nKEEP ',1)[0].encode()
        result['verify_sha256']=hashlib.sha256(blob).hexdigest()
        result['verify_bytes']=len(blob)
    if err.strip(): result['time_stderr']=err
    return result

def save(phase,result):
    with (OUT/(phase+'.jsonl')).open('a') as f: f.write(json.dumps(result,separators=(',',':'))+'\n')

def main():
    metadata=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),phase=A.phase,load_before=os.getloadavg(),
                  host=subprocess.check_output(['uname','-a'],text=True).strip(),
                  cpu=subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),
                  memory=int(subprocess.check_output(['sysctl','-n','hw.memsize'],text=True)),
                  versions={e:subprocess.check_output([cmd[0],'--version'],text=True).strip() for e,cmd in ENGINES.items() if e in ['node','bun']})
    metadata['worker_sha256']={e:hashlib.sha256(Path(cmd[0]).read_bytes()).hexdigest() for e,cmd in ENGINES.items() if e in ['perry','baseline']}
    metadata['node_v8']=subprocess.check_output([ENGINES['node'][0],'-p','process.versions.v8'],text=True).strip()
    (OUT/('host-'+A.phase+'.json')).write_text(json.dumps(metadata,indent=2)+'\n')
    if A.phase in ['verify','all']:
        for f in FIXTURES:
            for op in f['operations']:
                records=[one(e,f,op,1,verify=True) for e in ENGINES]
                expected=next(r for r in records if r['engine']=='node').get('verify_sha256')
                for r in records:
                    r['correct']=bool(expected) and r.get('verify_sha256')==expected and 'error' not in r
                    save('verify',r)
                print('VERIFY',f['name'],op,[(r['engine'],r['correct']) for r in records],flush=True)
    if A.phase in ['timing','all','tape']:
        rng=random.Random(9172)
        for f in FIXTURES:
            ops=f['operations'] if A.phase!='tape' else ['parse','sparse','scan','roundtrip']
            if A.phase=='tape' and f['kind']!='records_array': continue
            for op in ops:
                arms=[(e,None) for e in ENGINES] if A.phase!='tape' else [('perry',0),('perry',1)]
                rates=[]
                initial=2000 if f['bytes']<4096 else 2
                warm=5000 if f['bytes']<4096 else 2
                valid=[]
                for e,tape in arms:
                    r=one(e,f,op,initial,warm,tape=tape,timeout=30)
                    save('calibration',r)
                    if 'error' not in r:
                        rates.append(max(.00001,r['wall_ms']/initial));valid.append((e,tape))
                if not rates: continue
                # Same amount of work across engines within a cell. Aim >=150ms
                # fastest, <=1500ms slowest; wide gaps can prevent both.
                n=max(1,min(2000000,int(min(150/min(rates),1500/max(rates)))))
                for rep in range(A.repeat):
                    order=valid.copy();rng.shuffle(order)
                    for e,tape in order:
                        r=one(e,f,op,n,warm,tape=tape,timeout=60)
                        r['rep']=rep;r['bytes']=f['bytes']
                        save('tape' if A.phase=='tape' else 'timing',r)
                        print('RUN',f['name'],op,e,tape,rep,n,round(r.get('wall_ms',-1),2),r.get('error',''),flush=True)
    if A.phase in ['memory','all']:
        wanted={'tiny_object':200000,'small_record':100000,'records_array_1m':16,'records_object_1m':16,
                'records_array_8m':4,'records_object_8m':4,'long_string_1m':32,'unicode_1m':32,'wide_1m':16}
        for f in FIXTURES:
            if f['name'] not in wanted: continue
            for op in ['retain-parse','retain-stringify']:
                for n in [1,wanted[f['name']]]:
                    for rep in range(A.repeat):
                        for e in ENGINES:
                            r=one(e,f,op,n,0,timeout=90);r['rep']=rep;r['bytes']=f['bytes']
                            save('memory',r)
                            print('MEM',f['name'],op,e,n,round(r.get('rss_after',0)/1048576,1),r.get('error',''),flush=True)
    metadata['load_after']=os.getloadavg()
    metadata['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    (OUT/('host-'+A.phase+'.json')).write_text(json.dumps(metadata,indent=2)+'\n')

if __name__=='__main__': main()
