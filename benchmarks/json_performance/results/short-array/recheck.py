from pathlib import Path
import importlib.util,json,random,sys,time,hashlib,subprocess,os
root=Path.cwd();sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
manifest={f['name']:f for f in r.FIXTURES}
rows=[json.loads(s) for s in (root/'results/fastpaths/timing.jsonl').read_text().splitlines()]
iters={(x['fixture'],x['operation']):x['iterations'] for x in rows}
cases=[('inline',f['name'],op) for f in r.FIXTURES for op in ['parse','stringify']]
cases += [('empty',f,'parse') for f in ['empty_object','tiny_object','small_record','object_1k']]
cases += [('empty',f,'stringify') for f in ['tiny_object','small_record']]
cases += [('entry', f, 'parse') for f in ['records_array_1m','records_array_8m','heterogeneous_1m']]
cases += [('tape', 'escaped_1m', 'stringify'), ('tape', 'wide_1m', 'stringify'), ('tape', 'numbers_1m', 'parse')]
cases += [('count','escaped_1m','stringify'),('count','wide_1m','stringify'),('count','numbers_1m','parse'),('count','long_string_1m','parse'),('count','unicode_1m','parse'),('count','records_object_20m','parse')]
cases += [('plan', f, 'parse') for f in ['tiny_object','small_record','object_1k','records_object_1m','records_object_8m']]
cases += [('marker','escaped_1m','stringify'),('marker','records_array_16k','parse'),('marker','records_array_1m','parse')]
assert len(cases)==64
out=root/'results/recheck-short-array';out.mkdir()
references={'inline':'baseline-worker','marker':'marker-probe-worker','plan':'plan-scan-worker','count':'escaped-count-worker','empty':'empty-parse-worker','entry':'parse-entry-worker','tape':'tape-owned-worker'}
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
subprocess.run(['python3',str(root/'short_array_retention.py')],check=True)
