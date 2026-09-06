from pathlib import Path
import subprocess,os,json,hashlib
root=Path.cwd();out=root/'results/direct-profiles';out.mkdir();records=[]
for arm,name,n in [('baseline','numbers_1m',4000),('candidate','numbers_1m',4000),('candidate','long_string_1m',60000)]:
 worker=root/'.work'/('baseline-worker' if arm=='baseline' else 'worker');op='parse' if name=='numbers_1m' else 'stringify';case=arm+'-'+name+'-'+op
 env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
 with (out/(case+'.stdout')).open('w') as stdout,(out/(case+'.stderr')).open('w') as stderr:
  cmd=[str(worker),str(root/'.work/fixtures'/(name+'.json')),op,str(n),'2']
  p=subprocess.Popen(cmd,env=env,stdout=stdout,stderr=stderr)
  try:
   subprocess.run(['/usr/bin/sample',str(p.pid),'3','1','-file',str(out/(case+'.txt'))],capture_output=True,check=True,timeout=20)
   code=p.wait(timeout=180)
  except BaseException:p.kill();p.wait();raise
 records.append({'case':case,'command':cmd,'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'exit':code})
 (out/'provenance.json').write_text(json.dumps(records,indent=2)+'\n')
 print(case,code,flush=True);assert code==0
