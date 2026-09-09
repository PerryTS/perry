from pathlib import Path
import hashlib,json,os,signal,subprocess,time
root=Path.cwd(); out=root/'results/main-e722-location-profiles'; out.mkdir(exist_ok=False)
worker=root/'.work/main-e722/rotating-worker'; metadata={'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'source_commit':'e7223f700c8ce69c388210dab394ad7142550527','purpose':'stack locations, not performance timings','sample_seconds':2,'startup_seconds':0.3,'intentionally_terminated':True,'fixtures':[]}
for name in ['small_record','unicode_1m']:
 command=[str(worker),str(root/'.work/rotating'/name),'parse','1000000000','8','time','rotating']
 with (out/(name+'.stdout')).open('w') as stdout,(out/(name+'.stderr')).open('w') as stderr:
  process=subprocess.Popen(command,stdout=stdout,stderr=stderr,start_new_session=True)
  try:
   time.sleep(0.3)
   assert process.poll() is None
   sample=subprocess.run(['/usr/bin/sample',str(process.pid),'2','1','-mayDie','-file',str(out/(name+'.sample.txt'))],capture_output=True,text=True,timeout=15)
   (out/(name+'.sample.log')).write_text(sample.stdout+sample.stderr)
   assert sample.returncode==0
  finally:
   if process.poll() is None:process.terminate()
   try:process.wait(timeout=5)
   except subprocess.TimeoutExpired:process.kill();process.wait()
  metadata['fixtures'].append({'name':name,'command':command,'pid':process.pid,'expected_interrupted_exit':process.returncode})
 print('SAMPLED',name,flush=True)
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
