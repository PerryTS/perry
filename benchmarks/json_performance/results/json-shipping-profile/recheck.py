from pathlib import Path
import importlib.util,json,random,sys,time,hashlib,subprocess,os
root=Path(__file__).resolve().parent
sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
reference_rows=[json.loads(l) for l in (prior/'results/fastpaths/timing.jsonl').read_text().splitlines()]
iters={(x['fixture'],x['operation']):x['iterations'] for x in reference_rows}
out=root/'results/recheck';out.mkdir()
engines={'parent':[str(prior/'.work/worker')], 'checkpoint':[str(root/'.work/checkpoint')], 'candidate':[str(root/'.work/worker')], 'node':['/Users/perry/nodebin/node',str(root/'worker.js')], 'bun':['/Users/perry/.bun/bin/bun',str(root/'worker.js')]}
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(Path(v[0]).read_bytes()).hexdigest() for k,v in engines.items()},'versions':{k:subprocess.check_output([engines[k][0],'--version'],text=True).strip() for k in ['node','bun']},'repeats':5,'seed':96345,'note':'Parent is the 16-codegen-unit retained source; checkpoint and candidate use default release settings. Pinned 38-row call counts; default GC; fresh processes; CPU includes user and system; retain outputs measured separately.'},indent=2)+'\n')
r.ENGINES=dict(engines,node=['/Users/perry/nodebin/node',str(root/'worker.js')],bun=['/Users/perry/.bun/bin/bun',str(root/'worker.js')])
for f in r.FIXTURES:
 for op in ['parse','stringify']:
  rows=[r.one(e,f,op,1,verify=True) for e in r.ENGINES]
  expected=next(x['verify_sha256'] for x in rows if x['engine']=='node')
  for x in rows:
   x['correct']='error' not in x and x.get('verify_sha256')==expected
   with (out/'verify.jsonl').open('a') as dest:dest.write(json.dumps(x)+'\n')
   assert x['correct'],x
r.ENGINES=engines
rng=random.Random(96345)
for f in r.FIXTURES:
 for op in ['parse','stringify']:
  n=iters[f['name'],op];warm=5000 if f['bytes']<4096 else 2
  for rep in range(5):
   order=list(engines);rng.shuffle(order)
   for engine in order:
    row=r.one(engine,f,op,n,warm);row.update(rep=rep,bytes=f['bytes'])
    with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
    assert row['exit_code']==0 and 'error' not in row,row
  print('TIMING',f['name'],op,n,flush=True)
wanted={'tiny_object':200000,'small_record':100000,'records_array_1m':16,'records_object_1m':16,'records_array_8m':4,'records_object_8m':4,'long_string_1m':32,'unicode_1m':32,'wide_1m':16}
for f in r.FIXTURES:
 if f['name'] not in wanted:continue
 for op in ['retain-parse','retain-stringify']:
  for n in [1,wanted[f['name']]]:
   for rep in range(3):
    order=list(engines);rng.shuffle(order)
    for engine in order:
     row=r.one(engine,f,op,n,0,timeout=90);row.update(rep=rep,bytes=f['bytes'])
     with (out/'memory.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
     assert row['exit_code']==0 and 'error' not in row,row
 print('MEMORY',f['name'],flush=True)
print('COMPLETE',flush=True)
