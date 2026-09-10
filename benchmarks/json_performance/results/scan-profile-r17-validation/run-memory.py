#!/usr/bin/env python3
"""Instrumented lifetime diagnostics; never count these as speedup measurements."""
from pathlib import Path
import argparse,hashlib,json,os,signal,subprocess,time
w=Path(__file__).resolve().parent;bench=w.parents[1];p=argparse.ArgumentParser();p.add_argument('--results-dir',type=Path,required=True);args=p.parse_args();d=args.results_dir;d.mkdir(parents=True,exist_ok=False)
cases=json.loads((w/'memory-cases.json').read_text())['cases'];worker=w/'memory-worker';clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};records=[]
for c in cases:
 label=c['fixture']+'-'+c['operation']+'-'+c['mode']+'-'+str(c['iterations']);env=clean|{'PERRY_GC_DIAG':'1'}|({'PERRY_JSON_TAPE':'0'} if c['mode']=='main_direct' else {});cmd=[str(worker),str(bench/'.work/fixtures'/(c['fixture']+'.json')),c['operation'],str(c['iterations']),'2','verify'];started=time.monotonic();peak=0;stop=None
 with (d/(label+'.stdout')).open('wb') as out,(d/(label+'.stderr')).open('wb') as err:
  proc=subprocess.Popen(cmd,stdout=out,stderr=err,env=env,start_new_session=True)
  while proc.poll() is None:
   r=subprocess.run(['ps','-o','rss=','-p',str(proc.pid)],capture_output=True,text=True)
   try:rss=int(r.stdout.strip())*1024
   except ValueError:rss=0
   peak=max(peak,rss)
   if rss>2*1024**3 or time.monotonic()-started>30:
    stop='RSS limit 2 GiB' if rss>2*1024**3 else '30 second watchdog';os.killpg(proc.pid,signal.SIGTERM);break
   time.sleep(.1)
  try:proc.wait(timeout=3)
  except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);proc.wait()
 row={'case':c,'command':cmd,'env_overrides':{k:v for k,v in env.items() if k.startswith('PERRY_')},'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'exit_code':proc.returncode,'observed_peak_rss_bytes':peak,'stop_reason':stop,'elapsed_seconds':time.monotonic()-started,'diagnostic_only':True,'instrumented':True};records.append(row);(d/'memory-diagnostics.json').write_text(json.dumps(records,indent=2)+'\n');print(label,'exit',proc.returncode,'peakMiB',round(peak/1048576,2),flush=True);assert proc.returncode==0 and stop is None,row
