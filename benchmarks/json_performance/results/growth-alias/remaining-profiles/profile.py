from pathlib import Path
import subprocess,time,json,os,hashlib
root=Path.cwd();out=root/'results/remaining-profiles';out.mkdir()
worker=root/'.work/worker';env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
expected='c3f1ec3f9946b482b0373e032226fb17084460298e2ac1d030ff853c5869dc26'
assert hashlib.sha256(worker.read_bytes()).hexdigest()==expected
rows=[]
for fixture,operation,calls in [('wide_1m','parse',512),('heterogeneous_1m','stringify',3072)]:
 name=fixture+'-'+operation
 command=[str(worker),str(root/'.work/fixtures'/(fixture+'.json')),operation,str(calls),'2']
 with (out/(name+'.stdout')).open('w') as stdout,(out/(name+'.stderr')).open('w') as stderr:
  p=subprocess.Popen(command,env=env,stdout=stdout,stderr=stderr)
  time.sleep(1)
  sample=subprocess.run(['sample',str(p.pid),'3','1','-file',str(out/(name+'.sample.txt'))],capture_output=True,text=True)
  rc=p.wait(timeout=120)
 row={'purpose':'Quiet M1 sampling diagnostic. Sample shares are not exact CPU percentages and do not replace the complete CPU/RSS matrix.','command':command,'worker_sha256':expected,'worker_exit':rc,'sample_exit':sample.returncode,'sample_stderr':sample.stderr,'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'],text=True).strip(),'load_after':os.getloadavg()}
 rows.append(row);(out/'metadata.json').write_text(json.dumps(rows,indent=2)+'\n')
 assert rc==0 and sample.returncode==0
 print('PROFILED',name,flush=True)
