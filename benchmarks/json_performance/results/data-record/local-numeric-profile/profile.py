from pathlib import Path
import os,subprocess,json,hashlib,time
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');root=Path('/Users/amlug/projects/perry/codex-json-fastpaths/benchmarks/json_performance');out=art/'numeric-parse-local-profile';out.mkdir()
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};records=[]
for arm in ['unicode','data-record']:
 worker=art/(arm+'-worker');cmd=[str(worker),str(root/'.work/fixtures/numbers_1m.json'),'parse','4096','2']
 with (out/(arm+'.stdout')).open('w') as stdout,(out/(arm+'.stderr')).open('w') as stderr:
  p=subprocess.Popen(cmd,env=env,stdout=stdout,stderr=stderr)
  time.sleep(.5)
  sample=subprocess.run(['/usr/bin/sample',str(p.pid),'3','1','-file',str(out/(arm+'.sample.txt'))],capture_output=True,text=True,timeout=30)
  code=p.wait(timeout=180)
 row={'arm':arm,'command':cmd,'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'worker_exit':code,'sample_exit':sample.returncode,'sampler_stderr':sample.stderr,'purpose':'Busy local developer-host sample, not qualified CPU/RSS measurements'};records.append(row)
 (out/'metadata.json').write_text(json.dumps(records,indent=2)+'\n');print(arm,code,sample.returncode,flush=True)
 assert code==0 and sample.returncode==0
print('FINISHED',flush=True)
