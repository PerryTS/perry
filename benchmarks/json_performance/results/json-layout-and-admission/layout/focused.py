from pathlib import Path
import importlib.util,json,random,sys,hashlib
root=Path(__file__).resolve().parent
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
iters={(x['fixture'],x['operation']):x['iterations'] for x in map(json.loads,(prior/'results/fastpaths/timing.jsonl').read_text().splitlines())}
engines={'checkpoint':[str(prior/'.work/worker')],'stack':[str(Path.home()/'json-stack-plan-v4-20260907-codex/.work/worker')],'ordered_checkpoint':[str(root/'.work/defer2-ordered-worker')],'ordered_stack':[str(root/'.work/defer4-ordered-worker')]}
out=root/'results/focused';out.mkdir()
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(Path(v[0]).read_bytes()).hexdigest() for k,v in engines.items()},'seed':173499,'repeats':7},indent=2)+'\n')
wanted={('tiny_object','stringify'),('small_record','stringify'),('heterogeneous_1m','parse'),('records_array_16k','parse'),('records_array_1m','parse'),('records_array_8m','parse'),('numbers_1m','stringify'),('wide_1m','parse')}
rng=random.Random(173499)
r.ENGINES=dict(engines,node=['/Users/perry/nodebin/node',str(root/'worker.js')])
found=set()
for f in r.FIXTURES:
 for op in ['parse','stringify']:
  if (f['name'],op) not in wanted:continue
  found.add((f['name'],op));checks=[r.one(e,f,op,1,verify=True) for e in r.ENGINES];expected=checks[-1]['verify_sha256']
  for row in checks:
   row['correct']=row.get('verify_sha256')==expected and 'error' not in row
   with (out/'verify.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
   assert row['correct'],row
  for rep in range(7):
   order=list(engines);rng.shuffle(order)
   for e in order:
    row=r.one(e,f,op,iters[f['name'],op],5000 if f['bytes']<4096 else 2);row.update(rep=rep,bytes=f['bytes'])
    with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
    assert row['exit_code']==0 and 'error' not in row,row
  print('TIMING',f['name'],op,flush=True)
assert found==wanted,(found,wanted)
