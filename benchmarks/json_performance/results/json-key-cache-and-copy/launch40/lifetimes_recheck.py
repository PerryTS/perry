from pathlib import Path
import hashlib,json,os,random,re,subprocess,shutil
root=Path(__file__).resolve().parent
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
out=root/'results/lifetimes';out.mkdir()
engines={'fused':Path.home()/'json-fused-key-runtime-v35-20260907-codex/.work/worker-lifetime-candidate','previous':Path.home()/'json-depth-runtime-v22-20260907-codex/.work/worker-lifetime-candidate','scalar':Path.home()/'json-scalar-runtime-v31-20260907-codex/.work/worker-lifetime-candidate','candidate':Path.home()/'json-wide-key-runtime-v39-20260907-codex/.work/worker-lifetime-candidate'}
cases=[('small_record','stringify',mode,100000) for mode in ['discard','latest','retain']]
cases += [('records_object_20m','roundtrip','discard',24)]
cases += [('escaped_1m','parse',mode,32 if mode=='retain' else 100) for mode in ['discard','latest','retain']]
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in engines.items()},'cases':cases,'repeats':3,'seed':81134},indent=2)+'\n')
hashes={e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in engines.items()}
rng=random.Random(81134)
for fixture,op,mode,count in cases:
 for rep in range(3):
  order=list(engines);rng.shuffle(order)
  for engine in order:
   env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
   assert hashlib.sha256(engines[engine].read_bytes()).hexdigest()==hashes[engine]
   # Default GC, immutable executable, identical managed argv; no tracing.
   argv0=str(Path.home()/'json-escape-runtime-v18-20260907-codex/active')
   launcher=Path.home()/'json-launch-control-v25-20260907-codex/launch'
   cmd=[str(launcher),str(engines[engine]),argv0,str(prior/'.work/fixtures'/(fixture+'.json')),mode,str(count),op]
   p=subprocess.run(['/usr/bin/time','-l']+cmd,env=env,capture_output=True,text=True,timeout=180)
   assert p.returncode==0,(cmd,p.stderr[-2000:])
   row=json.loads(next(l[9:] for l in p.stdout.splitlines() if l.startswith('LIFETIME ')))
   row.update(engine=engine,fixture=fixture,rep=rep,trace=False,argv0=argv0,physical_executable=str(engines[engine]),launcher=str(launcher))
   if op=='parse':assert row['checksum']==count
   if mode in ['latest','retain']:
    actual=json.loads(next(l[7:] for l in p.stdout.splitlines() if l.startswith('VERIFY ')))
    if op=='stringify':actual=json.loads(actual)
    expected=json.loads((prior/'.work/fixtures'/(fixture+'.json')).read_text())
    assert actual==expected
    row['output_verified']=True
   row['peak_rss']=int(re.search(r'(\d+)\s+maximum resident set size',p.stderr).group(1))
   if False:
    events=[json.loads(l) for l in p.stderr.splitlines() if l.startswith('{')]
    events=[e for e in events if e.get('event')=='gc_cycle']
    name=f'{engine}-{fixture}-{op}-{count}.trace.jsonl'
    (out/name).write_text('\n'.join(json.dumps(e) for e in events)+'\n')
    row['cycles']=len(events);row['gc_pause_us']=sum(e['pause_us'] for e in events)
   with (out/'raw.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
 print(fixture,op,count,flush=True)

assert hashes=={e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in engines.items()}
