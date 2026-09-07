from pathlib import Path
import os,subprocess,json,random,hashlib,sys,importlib.util,re
root=Path(__file__).resolve().parent;out=root/'results';canonical=Path.home()/'json-escape-runtime-v18-20260907-codex';os.chdir(canonical)
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
r.ROOT=canonical
sources={'previous':Path.home()/'json-depth-runtime-v22-20260907-codex/.work/worker','scalar':Path.home()/'json-scalar-runtime-v31-20260907-codex/.work/worker','candidate':Path.home()/'json-fused-key-runtime-v35-20260907-codex/.work/worker'}
launcher=Path.home()/'json-launch-control-v25-20260907-codex/launch';argv0=str(canonical/'active');r.ENGINES={e:[str(launcher),str(f),argv0] for e,f in sources.items()};hashes={e:hashlib.sha256(f.read_bytes()).hexdigest() for e,f in sources.items()}
fixtures={f['name']:f for f in r.FIXTURES};rng=random.Random(831742);cases=[('empty_object','parse',20000000),('tiny_object','parse',10000000),('small_record','parse',2000000)]
(out/'metadata.json').write_text(json.dumps(dict(purpose='Targeted confirmation of small parse regressions; no tracing; never pooled into full matrix',hashes=hashes,launcher_sha256=hashlib.sha256(launcher.read_bytes()).hexdigest(),argv0=argv0,cases=cases,repeats=7,seed=831742),indent=2)+'\n')
for f,op,n in cases:
 for rep in range(7):
  order=list(sources);rng.shuffle(order)
  for e in order:
   assert hashes[e]==hashlib.sha256(sources[e].read_bytes()).hexdigest()
   row=r.one(e,fixtures[f],op,n,5000 if fixtures[f]['bytes']<4096 else 2);row.update(rep=rep,argv0=argv0,physical_executable=str(sources[e]),trace=False);assert row['exit_code']==0 and 'error' not in row
   with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
 print('TIMED',f,flush=True)
assert hashes=={e:hashlib.sha256(f.read_bytes()).hexdigest() for e,f in sources.items()}
print('COMPLETE',flush=True)
