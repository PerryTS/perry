"""Separate diagnostic samples. Never pool profiled CPU/RSS into benchmark results."""
from pathlib import Path
import hashlib,json,os,subprocess,time
home=Path.home();root=home/'json-fused-key-profile-v37-20260907-codex';root.mkdir(exist_ok=True)
canonical=home/'json-escape-runtime-v18-20260907-codex';launcher=home/'json-launch-control-v25-20260907-codex/launch'
workers={'scalar31':(home/'json-scalar-runtime-v31-20260907-codex/.work/worker','31366657092618b000e3d9506e3167b71aec7d79c5f0f2639614ce52019ad421'),'fused35':(home/'json-fused-key-runtime-v35-20260907-codex/.work/worker','dae7b63e865b7e1f0fdf02198397022d99320fcebb66f77354197eb98ec48d43')}
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
finally:
 if (lock/'owner').read_text().strip()==owner:(lock/'owner').unlink();lock.rmdir()
