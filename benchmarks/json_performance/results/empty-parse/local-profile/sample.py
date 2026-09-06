from pathlib import Path
import subprocess,time,json,os,hashlib
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');root=Path('/Users/amlug/projects/perry/codex-json-fastpaths/benchmarks/json_performance');out=art/'empty-parse-local-profile';out.mkdir()
worker=art/'empty-parse-worker';env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
command=[str(worker),str(root/'.work/fixtures/empty_object.json'),'parse','100000000','5000']
with (out/'worker.stdout').open('w') as stdout,(out/'worker.stderr').open('w') as stderr:
 p=subprocess.Popen(command,env=env,stdout=stdout,stderr=stderr)
 time.sleep(.5)
 sample=subprocess.run(['sample',str(p.pid),'3','1','-file',str(out/'empty_object.sample.txt')],capture_output=True,text=True)
 rc=p.wait(timeout=120)
meta={'purpose':'Local sampling diagnostic only; not quiet-window CPU/RSS evidence.','command':command,'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'worker_exit':rc,'sample_exit':sample.returncode,'sample_stderr':sample.stderr,'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'load_after':os.getloadavg()}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n');assert rc==0 and sample.returncode==0;print('PROFILED',meta['cpu'],flush=True)
