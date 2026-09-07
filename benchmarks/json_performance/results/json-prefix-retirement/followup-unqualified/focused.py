from pathlib import Path
import importlib.util,json,random,sys,hashlib
root=Path(__file__).resolve().parent
sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
ref=[json.loads(l) for l in (prior/'results/fastpaths/timing.jsonl').read_text().splitlines()]
iters={(x['fixture'],x['operation']):x['iterations'] for x in ref}
out=root/'results/focused';out.mkdir()
r.ENGINES={'checkpoint':[str(prior/'.work/worker')],'candidate':[str(root/'.work/worker')]}
cases=[('numbers_1m','parse'),('long_string_1m','parse'),('long_string_1m','stringify'),('unicode_1m','parse'),('unicode_1m','stringify'),('wide_1m','parse'),('records_object_20m','parse'),('small_record','parse')]
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(Path(v[0]).read_bytes()).hexdigest() for k,v in r.ENGINES.items()},'cases':cases,'repeats':7,'seed':81237,'order':'randomized ABBA quadruples','traced':False},indent=2)+'\n')
rng=random.Random(81237)
for name,op in cases:
 f=next(x for x in r.FIXTURES if x['name']==name);n=iters[name,op];warm=5000 if f['bytes']<4096 else 2
 for rep in range(7):
  order=list(r.ENGINES);rng.shuffle(order)
  for pos,e in enumerate(order+order[::-1]):
   row=r.one(e,f,op,n,warm);row.update(rep=rep,position=pos,bytes=f['bytes'])
   with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
   assert row['exit_code']==0 and 'error' not in row,row
 print('FOCUSED',name,op,flush=True)
