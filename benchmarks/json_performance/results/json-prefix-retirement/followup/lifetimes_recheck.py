from pathlib import Path
import hashlib,json,os,random,re,subprocess
root=Path(__file__).resolve().parent
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
out=root/'results/lifetimes';out.mkdir()
engines={'checkpoint':prior/'.work/worker-lifetime-candidate','candidate':root/'.work/worker-lifetime-candidate'}
cases=[('records_object_20m','parse','retain',4),('records_object_20m','stringify','retain',8),('records_object_20m','parse','latest',24)]
assert all(op != 'roundtrip' or mode == 'discard' for _,op,mode,_ in cases)
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in engines.items()},'cases':cases,'repeats':3,'seed':81124},indent=2)+'\n')
rng=random.Random(81124)
for fixture,op,mode,count in cases:
 for rep in range(3):
  order=list(engines);rng.shuffle(order)
  for engine in order:
   env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
   # Default GC; no tracing in timing trials.
   cmd=[str(engines[engine]),str(prior/'.work/fixtures'/(fixture+'.json')),mode,str(count),op]
   p=subprocess.run(['/usr/bin/time','-l']+cmd,env=env,capture_output=True,text=True,timeout=180)
   assert p.returncode==0,(cmd,p.stderr[-2000:])
   row=json.loads(next(l[9:] for l in p.stdout.splitlines() if l.startswith('LIFETIME ')))
   row.update(engine=engine,fixture=fixture,rep=rep,trace=False)
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
