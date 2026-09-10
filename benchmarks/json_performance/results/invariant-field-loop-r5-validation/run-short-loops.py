from pathlib import Path
import argparse, hashlib, json, os, random, statistics, subprocess, sys, time
work=Path(__file__).resolve().parent; bench=work.parents[1]
sys.path.insert(0,str(bench))
from run_dispatch_focus import run
p=argparse.ArgumentParser()
for arg in ['worker','baseline-worker','prior-worker','node','bun','results-dir']:p.add_argument('--'+arg,required=True)
a=p.parse_args();out=Path(a.results_dir);out.mkdir(parents=True,exist_ok=False)
engines={'perry':[str(Path(a.worker).resolve())],'baseline':[str(Path(a.baseline_worker).resolve())],'prior':[str(Path(a.prior_worker).resolve())],'node':[a.node,str(work/'short-loops.js')],'bun':[a.bun,str(work/'short-loops.js')]}
fixture='records_array_16k';iterations=1000000;warmup=5000;repeat=9
paths=[Path(c[0]) for k,c in engines.items() if k not in ['node','bun']]+[work/'short-loops.ts',work/'short-loops.js',Path(__file__),bench/'run_dispatch_focus.py',bench/'.work/fixtures'/ (fixture+'.json')]
meta={'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'load_before':os.getloadavg(),'iterations':iterations,'warmup':warmup,'repeat':repeat,'commands':engines,'versions':{e:subprocess.check_output([engines[e][0],'--version'],text=True).strip() for e in ['node','bun']},'hashes':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(out/'host.json').write_text(json.dumps(meta,indent=2)+'\n')
def save(phase,row):
 with (out/(phase+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
rng=random.Random(10039);summaries=[]
for trips in [1,8,64]:
 expected=run(engines['node'],fixture,iterations,warmup,operation=str(trips));save('verify',dict(expected,engine='node',operation=str(trips)))
 rows=[]
 for rep in range(repeat):
  order=list(engines);rng.shuffle(order)
  for engine in order:
   row=run(engines[engine],fixture,iterations,warmup,operation=str(trips));row.update(engine=engine,operation=str(trips),rep=rep)
   row['correct']=row['checksum']==expected['checksum'] and row['retained']==0
   save('timing',row)
   if not row['correct']:raise RuntimeError((engine,trips,row,expected))
   rows.append(row)
 for engine in engines:
  selected=[r for r in rows if r['engine']==engine]
  cpu=[(r['user_us']+r['system_us'])/iterations for r in selected];rss=[r['peak_rss']/1048576 for r in selected]
  summaries.append(dict(fixture=fixture,operation=str(trips),engine=engine,cpu_us=statistics.median(cpu),cpu_samples_us=cpu,peak_rss_mib=statistics.median(rss),peak_rss_samples_mib=rss))
 print('FINISHED short loop',trips,flush=True)
(out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
meta.update(finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),load_after=os.getloadavg());(out/'host.json').write_text(json.dumps(meta,indent=2)+'\n')
