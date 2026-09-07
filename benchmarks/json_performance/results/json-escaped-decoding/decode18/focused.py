from pathlib import Path
import importlib.util,json,random,sys,hashlib,subprocess,os
root=Path(__file__).resolve().parent
sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
utf16=Path.home()/'json-fused-utf16-v11-20260907-codex/.work'
sources={'checkpoint':utf16/'checkpoint','counter':utf16/'worker','inline':Path.home()/'json-escape-runtime-v17-20260907-codex/.work/worker','candidate':root/'.work/worker'}
hashes={e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in sources.items()}
r.ENGINES={e:[str(p)] for e,p in sources.items()}
r.ENGINES.update(node=['/Users/perry/nodebin/node',str(root/'worker.js')],bun=['/Users/perry/.bun/bin/bun',str(root/'worker.js')])
iters={(v['fixture'],v['operation']):v['iterations'] for v in map(json.loads,(prior/'results/fastpaths/timing.jsonl').read_text().splitlines())}
fixtures={v['name']:v for v in r.FIXTURES}
cases=[('escaped_1m','parse'),('escaped_1m','stringify'),('unicode_1m','parse'),('long_string_1m','parse'),('records_object_1m','parse'),('small_record','parse'),('small_record','stringify'),('records_array_16k','stringify'),('empty_object','parse')]
out=root/'results/focused';out.mkdir()
(out/'metadata.json').write_text(json.dumps({'hashes':hashes,'cases':cases,'seed':661382,'repeats':7,'versions':{e:subprocess.check_output([r.ENGINES[e][0],'--version'],text=True).strip() for e in ['node','bun']},'note':'Identical Perry invocation path, atomic hardlinks to immutable binaries. Fixed prior loop counts; default GC.'},indent=2)+'\n')
slot=root/'active';pending=root/'next-active'
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
 for rep in range(7):
  order=list(r.ENGINES);rng.shuffle(order)
  for engine in order:
   row=one(engine,f,op,n,warm);row.update(rep=rep,bytes=f['bytes'])
   with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
   assert row['exit_code']==0 and 'error' not in row,row
 print('TIMING',name,op,n,flush=True)
assert {e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in sources.items()}==hashes
print('COMPLETE',flush=True)
