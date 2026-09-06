from pathlib import Path
import subprocess,time,json,os,hashlib
root=Path.cwd();out=root/'results/large-record-profile';out.mkdir()
worker=root/'.work/worker';env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
command=[str(worker),str(root/'.work/fixtures/records_object_20m.json'),'stringify','128','2']
with (out/'worker.stdout').open('w') as stdout,(out/'worker.stderr').open('w') as stderr:
 p=subprocess.Popen(command,env=env,stdout=stdout,stderr=stderr)
 time.sleep(1)
 sample=subprocess.run(['sample',str(p.pid),'3','1','-file',str(out/'records_object_20m.sample.txt')],capture_output=True,text=True)
 rc=p.wait(timeout=120)
meta={'purpose':'Quiet M1 sampling diagnostic. Sample shares are not a measured GC CPU percentage or a replacement for the full CPU/RSS matrix.','command':command,'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'worker_exit':rc,'sample_exit':sample.returncode,'sample_stderr':sample.stderr,'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'load_after':os.getloadavg()}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n');assert rc==0 and sample.returncode==0;print('PROFILED',meta['cpu'],flush=True)
