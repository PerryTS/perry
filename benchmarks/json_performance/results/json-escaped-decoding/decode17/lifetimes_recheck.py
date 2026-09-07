from pathlib import Path
import hashlib,json,os,random,re,subprocess,shutil
root=Path(__file__).resolve().parent
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
out=root/'results/lifetimes';out.mkdir()
engines={'checkpoint':Path.home()/'json-fused-utf16-v11-20260907-codex/.work/checkpoint-lifetime','counter':Path.home()/'json-fused-utf16-v11-20260907-codex/.work/worker-lifetime-candidate','candidate':root/'.work/worker-lifetime-candidate'}
cases=[('small_record','stringify',mode,100000) for mode in ['discard','latest','retain']]
cases += [('wide_1m','parse',mode,8 if mode=='retain' else 60) for mode in ['discard','latest','retain']]
cases += [('records_object_20m','roundtrip','discard',n) for n in [7,8,9,16,24]]
cases += [('records_object_1m','stringify','discard',120)]
cases += [(f,'parse',mode,32 if mode=='retain' else 100) for f in ['unicode_1m','escaped_1m'] for mode in ['discard','latest','retain']]
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in engines.items()},'cases':cases,'repeats':3,'seed':81124},indent=2)+'\n')
rng=random.Random(81124)
for fixture,op,mode,count in cases:
 for rep in range(3):
  order=list(engines);rng.shuffle(order)
  for engine in order:
   env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
   # Default GC; no tracing in timing trials.
   slot=root/'active'
   pending=root/'next-active'
   assert not pending.exists()
   os.link(engines[engine],pending)
   if slot.exists() and os.path.samefile(slot,pending):pending.unlink()
   else:pending.replace(slot)
   assert os.path.samefile(engines[engine],slot)
   assert hashlib.sha256(slot.read_bytes()).hexdigest()==hashlib.sha256(engines[engine].read_bytes()).hexdigest()
   cmd=[str(slot),str(prior/'.work/fixtures'/(fixture+'.json')),mode,str(count),op]
   p=subprocess.run(['/usr/bin/time','-l']+cmd,env=env,capture_output=True,text=True,timeout=180)
   assert p.returncode==0,(cmd,p.stderr[-2000:])
   row=json.loads(next(l[9:] for l in p.stdout.splitlines() if l.startswith('LIFETIME ')))
   row.update(engine=engine,fixture=fixture,rep=rep,trace=False,executable_path=str(slot))
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
