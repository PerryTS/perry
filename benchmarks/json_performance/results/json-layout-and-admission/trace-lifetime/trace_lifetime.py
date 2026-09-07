from pathlib import Path
import json,os,subprocess,random,hashlib
root=Path(__file__).resolve().parent;out=root/'results/trace-lifetime';out.mkdir()
prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
engines={'checkpoint':prior/'.work/worker-lifetime-candidate','admission':Path.home()/'json-shared-admission-v5-20260907-codex/.work/worker-lifetime-candidate','direct_entry':root/'.work/worker-lifetime-candidate'}
(out/'metadata.json').write_text(json.dumps({'workers':{e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in engines.items()},'trace':True,'purpose':'Collection counts and allocation/retention mechanism only; instrumented CPU excluded from performance acceptance.'},indent=2)+'\n')
rng=random.Random(92157)
for rep in range(2):
 order=list(engines);rng.shuffle(order)
 for e in order:
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_TRACE']='1'
  cmd=[str(engines[e]),str(prior/'.work/fixtures/records_object_20m.json'),'discard','24','roundtrip']
  p=subprocess.run(cmd,env=env,capture_output=True,text=True,timeout=180);assert p.returncode==0,(e,p.stderr[-2000:])
  events=[json.loads(l) for l in p.stderr.splitlines() if l.startswith('{')];events=[x for x in events if x.get('event')=='gc_cycle'];(out/f'{e}-{rep}.trace.jsonl').write_text('\n'.join(json.dumps(x) for x in events)+'\n')
  row=json.loads(next(l[9:] for l in p.stdout.splitlines() if l.startswith('LIFETIME ')));row.update(engine=e,rep=rep,trace=True,cycles=len(events),pause_us=sum(x['pause_us'] for x in events))
  with (out/'raw.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
  print(e,rep,'cycles',row['cycles'],flush=True)
