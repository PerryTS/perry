from pathlib import Path
import importlib.util,json,random,sys,hashlib,subprocess,os
root=Path(__file__).resolve().parent
sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
utf16=Path.home()/'json-fused-utf16-v11-20260907-codex/.work'
sources={'checkpoint':utf16/'checkpoint','previous':Path.home()/'json-escape-runtime-v18-20260907-codex/.work/worker','depth22':Path.home()/'json-depth-runtime-v22-20260907-codex/.work/worker','candidate':root/'.work/worker'}
hashes={e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in sources.items()}
r.ENGINES={e:[str(p)] for e,p in sources.items()}
r.ENGINES.update(node=['/Users/perry/nodebin/node',str(root/'worker.js')],bun=['/Users/perry/.bun/bin/bun',str(root/'worker.js')])
iters={(v['fixture'],v['operation']):v['iterations'] for v in map(json.loads,(prior/'results/fastpaths/timing.jsonl').read_text().splitlines())}
fixtures={v['name']:v for v in r.FIXTURES}
cases=[(name,op) for name in fixtures for op in ['parse','stringify']]
out=root/'results/recheck';out.mkdir()
(out/'metadata.json').write_text(json.dumps({'hashes':hashes,'cases':cases,'seed':661382,'repeats':5,'versions':{e:subprocess.check_output([r.ENGINES[e][0],'--version'],text=True).strip() for e in ['node','bun']},'note':'Perry uses exact decode18 invocation path and fixture paths across both windows. Atomic hardlinks to immutable binaries, same-inode rename handled. Fixed prior counts, default GC.'},indent=2)+'\n')
canonical=Path.home()/'json-escape-runtime-v18-20260907-codex'
r.ROOT=canonical
slot=canonical/'active';pending=canonical/'next-active'
for e in ['node','bun']:r.ENGINES[e][1]=str(canonical/'worker.js')
original_one=r.one
def one(engine,*args,**kwargs):
 if engine not in sources:return original_one(engine,*args,**kwargs)
 assert not pending.exists()
 os.link(sources[engine],pending)
 # POSIX rename is a no-op if two hardlinks already name the same inode.
 if slot.exists() and os.path.samefile(pending,slot):pending.unlink()
 else:pending.replace(slot)
 assert os.path.samefile(slot,sources[engine]) and hashlib.sha256(slot.read_bytes()).hexdigest()==hashes[engine]
 before=r.ENGINES[engine];r.ENGINES[engine]=[str(slot)]
 try:
  row=original_one(engine,*args,**kwargs);row['executable_path']=str(slot);return row
 finally:r.ENGINES[engine]=before
rng=random.Random(661382)
for name,op in cases:
 f=fixtures[name]
 rows=[one(e,f,op,1,verify=True) for e in r.ENGINES]
 expected=next(v['verify_sha256'] for v in rows if v['engine']=='node')
 for row in rows:
  row['correct']='error' not in row and row.get('verify_sha256')==expected
  with (out/'verify.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
  assert row['correct'],row
 n=iters[name,op];warm=5000 if f['bytes']<4096 else 2
 for rep in range(5):
  order=list(r.ENGINES);rng.shuffle(order)
  for engine in order:
   row=one(engine,f,op,n,warm);row.update(rep=rep,bytes=f['bytes'])
   with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
   assert row['exit_code']==0 and 'error' not in row,row
 print('TIMING',name,op,n,flush=True)
wanted={'tiny_object':200000,'small_record':100000,'records_array_1m':16,'records_object_1m':16,'records_array_8m':4,'records_object_8m':4,'long_string_1m':32,'unicode_1m':32,'wide_1m':16,'escaped_1m':32}
for name,count in wanted.items():
 f=fixtures[name]
 for op in ['retain-parse','retain-stringify']:
  for n in [1,count]:
   for rep in range(3):
    order=list(sources);rng.shuffle(order)
    for engine in order:
     row=one(engine,f,op,n,0,timeout=90);row.update(rep=rep,bytes=f['bytes'])
     with (out/'memory.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
     assert row['exit_code']==0 and 'error' not in row,row
 print('MEMORY',name,flush=True)
assert {e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in sources.items()}==hashes
print('COMPLETE',flush=True)
