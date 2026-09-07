from pathlib import Path
import importlib.util,json,random,sys,hashlib,os,shutil
root=Path(__file__).resolve().parent;sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
prior=Path.home()/'json-deferred-gc-v2-20260907-codex';previous=Path.home()/'json-fused-utf16-v11-20260907-codex'
source={'checkpoint':previous/'.work/checkpoint','candidate':previous/'.work/worker'}
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
expected={'checkpoint':'6a376de244705cbe8485063d254973e3e99582b95e8b290ddb370deac3ebd835','candidate':'8c475d542d97db7fbe41490e97cfbcfee7c3eeca1ca7ec83320dbdd8114e55d0'}
assert {e:sha(p) for e,p in source.items()}==expected
identities={p.stat().st_ino for p in source.values()}
slot=root/'active';temporary=root/'next-active';scratch=root/'rewrite-image'
assert not scratch.exists()
shutil.copy2(source['checkpoint'],scratch)
assert scratch.stat().st_ino not in identities
ref=[json.loads(l) for l in (prior/'results/fastpaths/timing.jsonl').read_text().splitlines()]
iters={(x['fixture'],x['operation']):x['iterations'] for x in ref}
cases=[('empty_object','parse'),('small_record','stringify'),('records_array_16k','stringify'),('long_string_1m','parse'),('unicode_1m','parse')]
meta=dict(workers=expected,cases=cases,executable_path=str(slot),seed=91837,methods=['rewrite','copy','link'],method_repeats=5,paired_repeats=7,source_inodes={e:p.stat().st_ino for e,p in source.items()})
(root/'results/metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
r.ENGINES={'trial':[str(slot)]};rng=random.Random(meta['seed'])
def install(engine,method):
 temporary.unlink(missing_ok=True)
 if method=='rewrite':
  assert scratch.stat().st_ino not in identities
  shutil.copy2(source[engine],scratch)
  os.link(scratch,temporary)
 elif method=='copy':shutil.copy2(source[engine],temporary)
 else:os.link(source[engine],temporary)
 os.replace(temporary,slot)
 assert sha(slot)==expected[engine]
 if method=='link':assert os.path.samefile(slot,source[engine])
 return slot.stat().st_ino
for phase in ['same-binary-methods','paired-link']:
 for name,op in cases:
  f=next(x for x in r.FIXTURES if x['name']==name);n=iters[name,op];warm=5000 if f['bytes']<4096 else 2
  for rep in range(5 if phase=='same-binary-methods' else 7):
   order=list(meta['methods']) if phase=='same-binary-methods' else list(source);rng.shuffle(order)
   for position,label in enumerate(order+order[::-1]):
    engine='checkpoint' if phase=='same-binary-methods' else label;method=label if phase=='same-binary-methods' else 'link'
    inode=install(engine,method);row=r.one('trial',f,op,n,warm)
    row.update(engine=engine,method=method,phase=phase,rep=rep,position=position,executable_path=str(slot),inode=inode,worker_sha256=expected[engine])
    with (root/'results/raw.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
    assert row['exit_code']==0 and 'error' not in row,row
  print('FINISHED',phase,name,op,flush=True)
assert {e:sha(p) for e,p in source.items()}==expected
(root/'results/source-hashes-after.json').write_text(json.dumps({e:sha(p) for e,p in source.items()},indent=2)+'\n')
