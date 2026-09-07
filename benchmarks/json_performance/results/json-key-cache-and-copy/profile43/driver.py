"""Separate diagnostic samples. Never pool profiled CPU/RSS into benchmark results."""
from pathlib import Path
import hashlib,json,os,subprocess,time
home=Path.home();root=home/'json-copy-profile-v43-20260907-codex';root.mkdir(exist_ok=True)
canonical=home/'json-escape-runtime-v18-20260907-codex';launcher=home/'json-launch-control-v25-20260907-codex/launch'
workers={name:(home/path,pin) for name,(path,pin) in {'wide39': ('json-wide-key-runtime-v39-20260907-codex/.work/worker', '33f426d3d1da56742f36fae55a87df3fef6d2d8f94a0063796f67c9a5b117bb1'), 'copy41': ('json-short-copy-runtime-v41-20260907-codex/.work/worker', '843f5e63e43f25705d3ec0245e7c4b2a77c02389db37e19db6ae298dcec8ed12')}.items()}
lock=home/'bench.lock';lock.mkdir();owner=f'json-profile-{os.getpid()}';(lock/'owner').write_text(owner+'\n')
try:
 assert os.getloadavg()[0]<=2.5
 records=[]
 for name,(worker,pin) in workers.items():
  assert hashlib.sha256(worker.read_bytes()).hexdigest()==pin
  for fixture,op,n in [('small_record','stringify',30000000),('wide_1m','parse',600)]:
   stem=f'{name}-{fixture}-{op}';env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
   cmd=[str(launcher),str(worker),str(canonical/'active'),str(canonical/'.work/fixtures'/(fixture+'.json')),op,str(n),'0']
   with (root/(stem+'.stdout')).open('w') as output,(root/(stem+'.stderr')).open('w') as error:
    p=subprocess.Popen(cmd,cwd=canonical,env=env,stdout=output,stderr=error)
    time.sleep(.15)
    s=subprocess.run(['/usr/bin/sample',str(p.pid),'3','1','-file',str(root/(stem+'.sample.txt'))],capture_output=True,text=True,timeout=20)
    (root/(stem+'.sample.log')).write_text(s.stdout+s.stderr)
    try:result=p.wait(timeout=40)
    except subprocess.TimeoutExpired:p.kill();p.wait();raise
   assert result==0 and s.returncode==0,(stem,result,s.returncode)
   records.append(dict(arm=name,fixture=fixture,operation=op,iterations=n,worker_sha256=pin,argv=cmd,sample_exit=s.returncode,process_exit=result,diagnostic_only=True))
   (root/'metadata.json').write_text(json.dumps(records,indent=2)+'\n');print('sampled',stem,flush=True)
 traces=[]
 for name,(worker,pin) in workers.items():
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_TRACE']='1'
  cmd=[str(launcher),str(worker),str(canonical/'active'),str(canonical/'.work/fixtures/wide_1m.json'),'parse','36','2']
  p=subprocess.run(cmd,cwd=canonical,env=env,capture_output=True,text=True,timeout=60)
  assert p.returncode==0,(name,p.returncode)
  events=[json.loads(line) for line in p.stderr.splitlines() if line.startswith('{')]
  events=[e for e in events if e.get('event')=='gc_cycle']
  (root/(name+'-wide.trace.jsonl')).write_text('\n'.join(json.dumps(e) for e in events)+'\n')
  (root/(name+'-wide.trace.stdout')).write_text(p.stdout)
  traces.append({'arm':name,'worker_sha256':pin,'cycles':len(events),'full':sum(e['collection_kind']=='full' for e in events),'pointer_slots':sum(e['layout_scans']['pointer_slots_read'] for e in events),'diagnostic_only':True})
 (root/'traces.json').write_text(json.dumps(traces,indent=2)+'\n');print('trace summaries',traces,flush=True)

finally:
 if (lock/'owner').read_text().strip()==owner:(lock/'owner').unlink();lock.rmdir()
