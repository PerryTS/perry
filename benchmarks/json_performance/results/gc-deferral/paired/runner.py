from pathlib import Path
import importlib.util,json,random,sys,time,hashlib,subprocess,os
root=Path.cwd();sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
manifest={f['name']:f for f in r.FIXTURES}
rows=[json.loads(s) for s in (root/'results/fastpaths/timing.jsonl').read_text().splitlines()]
iters={(x['fixture'],x['operation']):x['iterations'] for x in rows}
cases=[('parent',f['name'],op) for f in r.FIXTURES for op in ['parse','stringify']]
assert len(cases)==38
out=root/'results/recheck-defer2';out.mkdir()
references={'parent':'baseline-worker'}
meta={'time_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'load_before':os.getloadavg(),'cases':cases,'repetitions':7,'notes':'Same worker entry, default GC, fresh processes. Call counts fixed from the full matrix. Randomized two-arm order per repetition. CPU includes user and system time.'}
meta['worker_sha256']={n:hashlib.sha256((root/'.work'/p).read_bytes()).hexdigest() for n,p in dict(references,candidate='worker').items()}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
rng=random.Random(24019)
for reference,name,op in cases:
 f=manifest[name];n=iters[name,op];warm=5000 if f['bytes']<4096 else 2
 r.ENGINES={'baseline':[str(root/'.work'/references[reference])],'perry':[str(root/'.work/worker')]}
 dest=out/(reference+'-'+name+'-'+op+'.jsonl')
 for rep in range(7):
  order=list(r.ENGINES);rng.shuffle(order)
  for engine in order:
   row=r.one(engine,f,op,n,warm);row.update(rep=rep,reference=reference,bytes=f['bytes'])
   with dest.open('a') as file:file.write(json.dumps(row,separators=(',',':'))+'\n')
   assert 'error' not in row and row['exit_code']==0,row
 print('CHECKED',reference,name,op,n,flush=True)
subprocess.run(['python3',str(root/'defer2_retention.py')],check=True)
