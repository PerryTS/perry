#!/usr/bin/env python3
"""Diagnostic mode comparison on one unchanged main binary; not candidate qualification."""
from pathlib import Path
import argparse,hashlib,json,os,random,shutil,statistics,subprocess,sys,time
w=Path(__file__).resolve().parent;sys.path.insert(0,str(w.parents[1]))
from run_dispatch_focus import run,ROOT
p=argparse.ArgumentParser();p.add_argument('--worker',type=Path,required=True);p.add_argument('--node',required=True);p.add_argument('--bun',required=True);p.add_argument('--results-dir',type=Path,required=True);args=p.parse_args()
d=args.results_dir;d.mkdir(parents=True,exist_ok=False);worker=args.worker.resolve();cases=json.loads((w/'mode-cases.json').read_text())['cases']
engines={'main_auto':['/usr/bin/env','-u','PERRY_JSON_TAPE',str(worker)],'main_direct':['/usr/bin/env','PERRY_JSON_TAPE=0',str(worker)],'node':[str(Path(shutil.which(args.node)).resolve()),str(w/'harness/worker.js')],'bun':[str(Path(shutil.which(args.bun)).resolve()),str(w/'harness/worker.js')]}
sha=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
meta={'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'load_before':os.getloadavg(),'source_commit':'1a9c0de6cb790d2467b0ca22a660870025179b37','diagnostic_only':True,'commands':engines,'worker_sha256':sha(worker),'node_version':subprocess.check_output([args.node,'--version'],text=True).strip(),'bun_version':subprocess.check_output([args.bun,'--version'],text=True).strip(),'sources':{str(x):sha(x) for x in [Path(__file__),w/'mode-cases.json',ROOT/'run_dispatch_focus.py',w/'harness/worker.ts',w/'harness/worker.js']},'cases':cases}
assert meta['node_version']=='v26.5.1' and meta['bun_version']=='1.3.14'
(d/'host.json').write_text(json.dumps(meta,indent=2)+'\n')
def save(phase,row):
 with (d/(phase+'.jsonl')).open('a') as out:out.write(json.dumps(row)+'\n')
rng=random.Random(1531);summary=[]
for c in cases:
 fixture,op,count,warmup,reps=[c[k] for k in ['fixture','operation','iterations','warmup','repetitions']]
 expected=run(engines['node'],fixture,7,8,True,op);expected.update(engine='oracle',operation=op);save('verify',expected)
 for engine,command in engines.items():
  r=run(command,fixture,7,8,True,op);r.update(engine=engine,operation=op);r['correct']=all(r[k]==expected[k] for k in ['checksum','retained','verify_sha256']);save('verify',r);assert r['correct'],(engine,fixture,op)
 rows=[];unit=expected['checksum']/15
 for rep in range(reps):
  order=list(engines);rng.shuffle(order)
  for engine in order:
   r=run(engines[engine],fixture,count,warmup,operation=op);r.update(engine=engine,operation=op,rep=rep);save('timing',r);assert r['checksum']==unit*(count+warmup) and r['retained']==0,(engine,fixture,op);rows.append(r)
 for engine in engines:
  chosen=[r for r in rows if r['engine']==engine];cpu=[(r['user_us']+r['system_us'])/count for r in chosen];rss=[r['peak_rss']/1048576 for r in chosen]
  summary.append({'fixture':fixture,'operation':op,'engine':engine,'iterations':count,'warmup':warmup,'repetitions':reps,'cpu_samples_us':cpu,'cpu_us':statistics.median(cpu),'peak_rss_samples_mib':rss,'peak_rss_mib':statistics.median(rss)})
 print('DONE',fixture,op,flush=True)
(d/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');meta['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime());meta['load_after']=os.getloadavg();(d/'host.json').write_text(json.dumps(meta,indent=2)+'\n')
