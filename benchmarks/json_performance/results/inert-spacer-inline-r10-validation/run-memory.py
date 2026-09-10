#!/usr/bin/env python3
"""Original retained-output cases, with interleaving and complete last-output checks."""
from pathlib import Path
import argparse,hashlib,json,os,random,statistics,subprocess,sys,time
w=Path(__file__).resolve().parent;bench=w.parents[1];sys.path.insert(0,str(bench))
from run_dispatch_focus import run
p=argparse.ArgumentParser()
for arg in ['worker','baseline-worker','node','bun','results-dir']:p.add_argument('--'+arg,required=True)
p.add_argument('--repeat',type=int,default=7);a=p.parse_args();assert a.repeat>0
out=Path(a.results_dir);out.mkdir(parents=True,exist_ok=False)
engines={'perry':[str(Path(a.worker).resolve())],'baseline':[str(Path(a.baseline_worker).resolve())],
         'node':[a.node,str(w/'harness/worker.js')],'bun':[a.bun,str(w/'harness/worker.js')]}
wanted={'tiny_object':200000,'small_record':100000,'records_array_1m':16,'records_object_1m':16,
        'records_array_8m':4,'records_object_8m':4,'long_string_1m':32,'unicode_1m':32,'wide_1m':16}
paths=[Path(c[0]) for e,c in engines.items() if e in ['perry','baseline']]+[w/'harness/worker.ts',w/'harness/worker.js',Path(__file__),bench/'run_dispatch_focus.py']+[bench/'.work/fixtures'/(n+'.json') for n in wanted]
meta={'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'load_before':os.getloadavg(),
      'repeat':a.repeat,'warmup':0,'counts':wanted,'commands':engines,
      'versions':{e:subprocess.check_output([engines[e][0],'--version'],text=True).strip() for e in ['node','bun']},
      'hashes':{str(f):hashlib.sha256(f.read_bytes()).hexdigest() for f in paths},
      'scope':'Retained count and full last output verified; matches original retained worker semantics. Peak is process RSS; rss_after is measured before verification.'}
(out/'host.json').write_text(json.dumps(meta,indent=2)+'\n')
def save(phase,row):
 with (out/(phase+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
rng=random.Random(10040);summaries=[]
for fixture,maximum in wanted.items():
 for operation in ['retain-parse','retain-stringify']:
  for count in [1,maximum]:
   expected=run(engines['node'],fixture,count,0,verify=True,operation=operation)
   save('verify',dict(expected,engine='node',operation=operation))
   assert expected['retained']==count
   for engine,cmd in engines.items():
    row=run(cmd,fixture,count,0,verify=True,operation=operation)
    row.update(engine=engine,operation=operation)
    row['correct']=all(row[k]==expected[k] for k in ['checksum','retained','verify_sha256'])
    save('verify',row);assert row['correct'],row
   rows=[]
   for rep in range(a.repeat):
    order=list(engines);rng.shuffle(order)
    for engine in order:
     row=run(engines[engine],fixture,count,0,operation=operation);row.update(engine=engine,operation=operation,rep=rep)
     assert row['checksum']==expected['checksum'] and row['retained']==count,row
     save('memory',row);rows.append(row)
   for engine in engines:
    chosen=[r for r in rows if r['engine']==engine];assert len(chosen)==a.repeat
    cpu=[(r['user_us']+r['system_us'])/count for r in chosen]
    peak=[r['peak_rss']/1048576 for r in chosen];after=[r['rss_after']/1048576 for r in chosen]
    summaries.append(dict(fixture=fixture,operation=operation,iterations=count,engine=engine,repetitions=a.repeat,
                         cpu_us=statistics.median(cpu),cpu_samples_us=cpu,peak_rss_mib=statistics.median(peak),peak_rss_samples_mib=peak,
                         rss_after_mib=statistics.median(after),rss_after_samples_mib=after))
   print('FINISHED retained',fixture,operation,count,flush=True)
(out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
meta.update(finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),load_after=os.getloadavg())
(out/'host.json').write_text(json.dumps(meta,indent=2)+'\n')
