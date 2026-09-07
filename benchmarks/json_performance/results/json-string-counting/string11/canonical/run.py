from pathlib import Path
import os,subprocess,json,hashlib,random,re,statistics,shutil
root=Path(__file__).resolve().parent
base=root.parent;prior=Path.home()/'json-deferred-gc-v2-20260907-codex'
workers={'checkpoint':base/'.work/checkpoint-lifetime','candidate':base/'.work/worker-lifetime-candidate'}
slot=root/'active';env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
fixture=prior/'.work/fixtures/records_object_20m.json';rows=[];rng=random.Random(81428)
meta=dict(workers={e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in workers.items()},fixture_sha256=hashlib.sha256(fixture.read_bytes()).hexdigest(),slot=str(slot),seed=81428,traced=False)
(root/'results/metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
def run(engine,binary,group,position,phase):
 p=subprocess.run(['/usr/bin/time','-l',str(binary),str(fixture),'discard','24','roundtrip'],env=env,text=True,capture_output=True,timeout=180)
 assert p.returncode==0,p.stderr[-1000:]
 r=json.loads(next(l[9:] for l in p.stdout.splitlines() if l.startswith('LIFETIME ')))
 r.update(engine=engine,group=group,position=position,phase=phase,executable_path=str(binary),peak_rss=int(re.search(r'(\d+)\s+maximum resident set size',p.stderr).group(1)))
 r['instructions']=int(re.search(r'(\d+)\s+instructions retired',p.stderr).group(1));r['time_stderr']=p.stderr
 rows.append(r);(root/'results/raw.json').write_text(json.dumps(rows,indent=2)+'\n')
for rep in range(5):
 order=list(workers);rng.shuffle(order)
 for position,engine in enumerate(order+order[::-1]):
  shutil.copy2(workers[engine],slot)
  assert hashlib.sha256(slot.read_bytes()).hexdigest()==meta['workers'][engine]
  run(engine,slot,rep,position,'canonical')
 print('CANONICAL',rep,flush=True)
# Same bytes, two invocation paths. This tests the path hypothesis directly.
# Both paths already exist and resolve to the same immutable reference binary.
paths={'direct':base/'.work/checkpoint-lifetime','nested':base/'followup/.work/checkpoint-lifetime'}
assert all(hashlib.sha256(p.read_bytes()).hexdigest()==meta['workers']['checkpoint'] for p in paths.values())
for rep in range(3):
 order=list(paths);rng.shuffle(order)
 for position,label in enumerate(order+order[::-1]):run(label,paths[label],rep,position,'same-binary-path-control')
 print('PATH_CONTROL',rep,flush=True)
trace_rows=[]
for label,original,binary in [('checkpoint-canonical',workers['checkpoint'],slot),('candidate-canonical',workers['candidate'],slot),('checkpoint-direct',workers['checkpoint'],paths['direct']),('checkpoint-nested',workers['checkpoint'],paths['nested'])]:
 if binary==slot:shutil.copy2(original,slot)
 p=subprocess.run([str(binary),str(fixture),'discard','24','roundtrip'],env=dict(env,PERRY_GC_TRACE='1'),text=True,capture_output=True,timeout=180)
 assert p.returncode==0
 (root/'results'/(label+'.trace.stdout')).write_text(p.stdout);(root/'results'/(label+'.trace.stderr')).write_text(p.stderr)
 events=[json.loads(l) for l in p.stderr.splitlines() if l.startswith('{')];events=[e for e in events if e.get('event')=='gc_cycle']
 trace_rows.append(dict(label=label,worker_sha256=hashlib.sha256(original.read_bytes()).hexdigest(),executable_path=str(binary),cycles=len(events),full=sum(e['collection_kind']=='full' for e in events),pointer_slots=sum(e['layout_scans']['pointer_slots_read'] for e in events)))
 (root/'results/trace-summary.json').write_text(json.dumps(trace_rows,indent=2)+'\n')
 print('TRACE',label,len(events),flush=True)
