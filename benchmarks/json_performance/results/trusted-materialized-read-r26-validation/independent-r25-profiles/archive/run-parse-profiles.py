#!/usr/bin/env python3
"""Bounded sampling diagnostics, separate from uninstrumented mode comparisons."""
from pathlib import Path
import argparse,hashlib,json,os,signal,subprocess,time
w=Path(__file__).resolve().parent;bench=w.parents[1]
p=argparse.ArgumentParser();p.add_argument('--results-dir',type=Path,required=True);a=p.parse_args();d=a.results_dir;d.mkdir(parents=True,exist_ok=False)
cases=json.loads((w/'profile-cases.json').read_text())['cases'];clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};records=[]
for c in cases:
 label=c['fixture']+'-'+c['operation']+'-'+c['mode'];env=clean;worker=w/'r25-rotating-worker'
 cmd=[str(worker),str(bench/'.work/rotating'/c['fixture']),c['operation'],str(c['iterations']),str(c['warmup']),'verify','rotating']
 with (d/(label+'.stdout')).open('wb') as out,(d/(label+'.stderr')).open('wb') as err:
  proc=subprocess.Popen(cmd,env=env,stdout=out,stderr=err,start_new_session=True);started=time.monotonic();time.sleep(0.5);sample_cmd=['/usr/bin/sample',str(proc.pid),'1','1','-mayDie','-file',str(d/(label+'.sample.txt'))]
  with (d/(label+'.sampler.stdout')).open('wb') as so,(d/(label+'.sampler.stderr')).open('wb') as se:
   sampler=subprocess.Popen(sample_cmd,stdout=so,stderr=se);peak=0;stop_reason=None
   while proc.poll() is None:
    r=subprocess.run(['ps','-o','rss=','-p',str(proc.pid)],capture_output=True,text=True)
    try:rss=int(r.stdout.strip())*1024
    except ValueError:rss=0
    peak=max(peak,rss)
    if rss>2*1024**3 or time.monotonic()-started>20:
     stop_reason='RSS limit 2 GiB' if rss>2*1024**3 else '20 second watchdog';os.killpg(proc.pid,signal.SIGTERM);break
    time.sleep(0.1)
   try:proc.wait(timeout=3)
   except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);proc.wait()
   try:sampler.wait(timeout=15)
   except subprocess.TimeoutExpired:sampler.kill();sampler.wait();stop_reason=stop_reason or 'sampler timeout'
 row={'case':c,'worker_command':cmd,'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'env_overrides':{},'sample_command':sample_cmd,'worker_exit':proc.returncode,'sampler_exit':sampler.returncode,'observed_peak_rss_bytes':peak,'elapsed_seconds':time.monotonic()-started,'stop_reason':stop_reason,'diagnostic_only':True};records.append(row);(d/'profiles.json').write_text(json.dumps(records,indent=2)+'\n');print(label,'worker',proc.returncode,'sampler',sampler.returncode,'peak MiB',round(peak/1048576,2),flush=True)
 assert stop_reason is None and proc.returncode==0 and sampler.returncode==0,row
 report=d/(label+'.sample.txt');assert report.exists() and 'Call graph:' in report.read_text(),label
