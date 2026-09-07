from pathlib import Path
import os,subprocess,json,random,hashlib,sys,importlib.util,re
root=Path(__file__).resolve().parent;out=root/'results';canonical=Path.home()/'json-escape-runtime-v18-20260907-codex';os.chdir(canonical)
sys.argv=['run.py'];spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
r.ROOT=canonical
sources={'previous':Path.home()/'json-depth-runtime-v22-20260907-codex/.work/worker','candidate':Path.home()/'json-structural-runtime-v24-20260907-codex/.work/worker'}
launcher=Path.home()/'json-launch-control-v25-20260907-codex/launch';argv0=str(canonical/'active');r.ENGINES={e:[str(launcher),str(f),argv0] for e,f in sources.items()};hashes={e:hashlib.sha256(f.read_bytes()).hexdigest() for e,f in sources.items()}
fixtures={f['name']:f for f in r.FIXTURES};rng=random.Random(831742);cases=[('null','parse',20000000),('string_a','parse',20000000),('small_record','parse',2000000),('escaped_1m','parse',82)]
(out/'metadata.json').write_text(json.dumps(dict(purpose='Targeted longer-loop scalar recheck and separate GC/RSS diagnostics; never pooled into full matrix',hashes=hashes,launcher_sha256=hashlib.sha256(launcher.read_bytes()).hexdigest(),argv0=argv0,cases=cases,repeats=7,seed=831742),indent=2)+'\n')
for f,op,n in cases:
 for rep in range(7):
  order=list(sources);rng.shuffle(order)
  for e in order:
   row=r.one(e,fixtures[f],op,n,5000 if fixtures[f]['bytes']<4096 else 2);row.update(rep=rep,argv0=argv0,physical_executable=str(sources[e]),trace=False);assert row['exit_code']==0 and 'error' not in row
   with (out/'timing.jsonl').open('a') as dest:dest.write(json.dumps(row)+'\n')
 print('TIMED',f,flush=True)
traces=[]
for rep in range(2):
 order=list(sources);rng.shuffle(order)
 for e in order:
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_TRACE']='1'
  cmd=r.ENGINES[e]+[str(canonical/'.work/fixtures/escaped_1m.json'),'parse','82','2']
  p=subprocess.run(['/usr/bin/time','-l']+cmd,env=env,capture_output=True,text=True,timeout=120);assert p.returncode==0
  events=[json.loads(l) for l in p.stderr.splitlines() if l.startswith('{')];events=[x for x in events if x.get('event')=='gc_cycle']
  name=f'{e}-{rep}.trace.jsonl';(out/name).write_text('\n'.join(json.dumps(x) for x in events)+'\n')
  traces.append(dict(engine=e,rep=rep,cycles=len(events),full=sum(x['collection_kind']=='full' for x in events),pointer_slots=sum(x['layout_scans']['pointer_slots_read'] for x in events),peak_rss=int(re.search(r'(\d+)\s+maximum resident set size',p.stderr).group(1)),trace_file=name))
(out/'traces.json').write_text(json.dumps(traces,indent=2)+'\n')
assert hashes=={e:hashlib.sha256(f.read_bytes()).hexdigest() for e,f in sources.items()}
print('COMPLETE',flush=True)
