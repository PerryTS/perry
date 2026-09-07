from pathlib import Path
import hashlib,json,os,random,re,subprocess
root=Path(__file__).resolve().parent
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
out=root/'results/diagnosis';out.mkdir()
engines={name:prior/'.work'/('worker-lifetime-'+name) for name in ['parent','candidate']}
cases=[('small_record','stringify','discard',100000),('small_record','stringify','discard',1000000),('object_1k','parse','retain',20000),('wide_1m','parse','discard',60)]
cases += [('records_object_20m','roundtrip','discard',n) for n in [7,8,9,16,24]]
(out/'metadata.json').write_text(json.dumps({'workers':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in engines.items()},'cases':cases,'repeats':3,'seed':81124},indent=2)+'\n')
rng=random.Random(81124)
for fixture,op,mode,count in cases:
 for rep in range(3):
  order=list(engines);rng.shuffle(order)
  for engine in order:
   env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
   if rep==0:env['PERRY_GC_TRACE']='1'
   cmd=[str(engines[engine]),str(prior/'.work/fixtures'/(fixture+'.json')),mode,str(count),op]
   p=subprocess.run(['/usr/bin/time','-l']+cmd,env=env,capture_output=True,text=True,timeout=180)
   assert p.returncode==0,(cmd,p.stderr[-2000:])
   row=json.loads(next(l[9:] for l in p.stdout.splitlines() if l.startswith('LIFETIME ')))
   row.update(engine=engine,fixture=fixture,rep=rep,trace=rep==0)
   row['peak_rss']=int(re.search(r'(\d+)\s+maximum resident set size',p.stderr).group(1))
   if rep==0:
    events=[json.loads(l) for l in p.stderr.splitlines() if l.startswith('{')]
    events=[e for e in events if e.get('event')=='gc_cycle']
    name=f'{engine}-{fixture}-{op}-{count}.trace.jsonl'
    (out/name).write_text('\n'.join(json.dumps(e) for e in events)+'\n')
    row['cycles']=len(events);row['gc_pause_us']=sum(e['pause_us'] for e in events)
   with (out/'raw.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
 print(fixture,op,count,flush=True)
