"""Separate diagnostic processes; never feed traced CPU/RSS into timing."""
from pathlib import Path
import subprocess, json

art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts')
host='perry@perry-macos.local'
pins={arm:json.loads((art/(arm+'-provenance.json')).read_text())['workers']['worker']['sha256']
      for arm in ['copy41','keys45','typed48']}
script=r'''
from pathlib import Path
import subprocess,json,hashlib,os
home=Path.home();out=home/'json-key-lifetime-trace-v50-20260907-codex'
lock=home/'bench.lock';lock.mkdir()
(lock/'owner').write_text('codex JSON trace50 diagnostic\n')
try:
 out.mkdir()
 pins=PINS
 paths={'copy41':'json-short-copy-runtime-v41-20260907-codex','typed48':'json-typed-key-runtime-v48-20260907-codex','keys45':'json-key-lifetime-runtime-v45-20260907-codex'}
 canonical=home/'json-escape-runtime-v18-20260907-codex'
 launcher=home/'json-launch-control-v25-20260907-codex/launch'
 assert hashlib.sha256(launcher.read_bytes()).hexdigest()=='099ff6af8d7540e1b702c83d3cbb3907149cfd6f9050d5f976b7cf1f5db7feb1'
 metadata=[]
 for arm,remote in paths.items():
  worker=home/remote/'.work/worker';assert hashlib.sha256(worker.read_bytes()).hexdigest()==pins[arm]
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_TRACE']='1'
  cmd=[str(launcher),str(worker),str(canonical/'active'),str(canonical/'.work/fixtures/wide_1m.json'),'parse','36','2']
  p=subprocess.run(cmd,cwd=canonical,env=env,text=True,capture_output=True,timeout=180)
  (out/(arm+'-stdout.txt')).write_text(p.stdout)
  (out/(arm+'-stderr.txt')).write_text(p.stderr)
  assert p.returncode==0,(arm,p.returncode,p.stderr[-2000:])
  events=[json.loads(line) for line in p.stderr.splitlines() if line.startswith('{')]
  events=[e for e in events if e.get('event')=='gc_cycle'];assert events
  (out/(arm+'-wide.trace.jsonl')).write_text('\n'.join(json.dumps(e) for e in events)+'\n')
  metadata.append({'arm':arm,'worker_sha256':pins[arm],'diagnostic_only':True,'process_exit':p.returncode,'command':cmd,'cwd':str(canonical),'iterations':36,'warmup':2,'cycles':len(events),'full':sum(e['collection_kind']=='full' for e in events),'pointer_slots':sum(e['layout_scans']['pointer_slots_read'] for e in events),'last_longlived_bytes':events[-1]['arena_bytes']['after']['longlived']['in_use_bytes']})
  print(json.dumps(metadata[-1]),flush=True)
 (out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
finally:
 (lock/'owner').unlink();lock.rmdir()
'''.replace('PINS',repr(pins))
subprocess.run(['ssh',host,'python3 -'],input=script,text=True,check=True)
(art/'trace50').mkdir()
subprocess.run(['scp','-r',host+':json-key-lifetime-trace-v50-20260907-codex/.',str(art/'trace50')+'/'],check=True)
