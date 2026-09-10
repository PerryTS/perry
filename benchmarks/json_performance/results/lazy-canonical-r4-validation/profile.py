from pathlib import Path
import hashlib,json,os,subprocess,time
root=Path('/Users/amlug/projects/perry/json-merged-pr10022');os.chdir(root)
b=Path('benchmarks/json_performance'); w=b/'.work/lazy-canonical-r4';d=w/'location-profile';d.mkdir(exist_ok=False)
worker=w/'worker';args=[str(worker),str(b/'.work/fixtures/records_array_1m.json'),'roundtrip','1000000','8']
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
with (d/'worker.stdout').open('wb') as out,(d/'worker.stderr').open('wb') as err:
 p=subprocess.Popen(args,env=clean,stdout=out,stderr=err)
 try:
  time.sleep(.5)
  assert p.poll() is None
  sample=['/usr/bin/sample',str(p.pid),'2','1','-file',str(d/'sample.txt')]
  r=subprocess.run(sample,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=15)
  (d/'sample.log').write_bytes(r.stdout);assert r.returncode==0
 finally:
  p.terminate();p.wait(timeout=10)
(d/'provenance.json').write_text(json.dumps({'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'command':args,'sample_command':sample,'worker_intentionally_terminated':True,'worker_returncode':p.returncode,'diagnostic_only':True,'host':subprocess.check_output(['uname','-a'],text=True).strip()},indent=2)+'\n')
