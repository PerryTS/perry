from pathlib import Path
import subprocess,hashlib,os,json,time,signal
root=Path(__file__).resolve().parent
sources={'counter':Path.home()/'json-fused-utf16-v11-20260907-codex/.work/worker','candidate':root.parent/'.work/worker'}
fixtures=Path.home()/'json-deferred-gc-v2-20260907-codex/.work/fixtures'
hashes={e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in sources.items()}
(root/'results/metadata.json').write_text(json.dumps({'sha256':hashes,'note':'Separate three-second native sampling; intentionally terminated workers. Diagnostic only, not timing qualification. Both arms at one invocation path.'},indent=2)+'\n')
rows=[]
for fixture,op in [('long_string_1m','parse'),('escaped_1m','parse'),('small_record','stringify')]:
 for arm,source in sources.items():
  slot=root/'active';pending=root/'next-active'
  assert not pending.exists();os.link(source,pending)
  if slot.exists() and os.path.samefile(slot,pending):pending.unlink()
  else:pending.replace(slot)
  assert hashlib.sha256(slot.read_bytes()).hexdigest()==hashes[arm]
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
  name=f'{arm}-{fixture}-{op}'
  with (root/'results'/(name+'.stdout')).open('w') as out,(root/'results'/(name+'.stderr')).open('w') as err:
   worker=subprocess.Popen([str(slot),str(fixtures/(fixture+'.json')),op,'2000000000','2'],env=env,stdout=out,stderr=err)
   try:
    time.sleep(.2);assert worker.poll() is None
    sample=subprocess.run(['sample',str(worker.pid),'3','1','-file',str(root/'results'/(name+'.sample.txt'))],capture_output=True,text=True,timeout=15)
    assert sample.returncode==0,(name,sample.stderr)
   finally:
    if worker.poll() is None:worker.terminate()
    worker.wait(timeout=10)
  assert worker.returncode==-signal.SIGTERM
  rows.append({'arm':arm,'fixture':fixture,'operation':op,'sample_exit':sample.returncode,'worker_exit':worker.returncode,'executable_path':str(slot)})
  (root/'results/summary.json').write_text(json.dumps(rows,indent=2)+'\n')
  print(name,flush=True)
assert {e:hashlib.sha256(p.read_bytes()).hexdigest() for e,p in sources.items()}==hashes
