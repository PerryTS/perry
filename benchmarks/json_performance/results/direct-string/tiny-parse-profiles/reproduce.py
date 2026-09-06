from pathlib import Path
import os,subprocess,json,hashlib,time
root=Path.cwd();out=root/'results/tiny-parse-profiles';out.mkdir()
worker=root/'.work/worker'
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
records=[]
for name in ['null','tiny_object']:
 fixture=root/'.work/fixtures'/(name+'.json')
 with (out/(name+'.stdout')).open('w') as stdout, (out/(name+'.stderr')).open('w') as stderr:
  cmd=[str(worker),str(fixture),'parse','100000000','8']
  p=subprocess.Popen(cmd,env=env,stdout=stdout,stderr=stderr)
  try:
   subprocess.run(['/usr/bin/sample',str(p.pid),'3','1','-file',str(out/(name+'.txt'))],check=True,capture_output=True,text=True,timeout=20)
   code=p.wait(timeout=180)
  except BaseException:
   p.kill();p.wait();raise
 records.append({'fixture':name,'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'command':cmd,'exit':code,'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest()})
 (out/'provenance.json').write_text(json.dumps(records,indent=2)+'\n')
 print(name,code,flush=True)
 assert code==0
