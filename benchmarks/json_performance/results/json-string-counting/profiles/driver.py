from pathlib import Path
import subprocess,time,json,os,hashlib,signal
root=Path(__file__).resolve().parent;out=root/'results';fixtures=Path.home()/'json-deferred-gc-v2-20260907-codex/.work/fixtures'
workers={'iteration16':Path.home()/'json-deferred-gc-v2-20260907-codex/.work/worker','release1':Path.home()/'json-shipping-profile-v9-20260907-codex/.work/checkpoint'}
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};results=[]
for arm,worker in workers.items():
 for name,iterations in [('unicode_1m',1520),('escaped_1m',82)]:
  label=arm+'-'+name
  cmd=[str(worker),str(fixtures/(name+'.json')),'parse',str(iterations),'2']
  p=subprocess.run(cmd,env=dict(env,PERRY_GC_TRACE='1'),capture_output=True,text=True,timeout=90)
  (out/(label+'.trace.stdout')).write_text(p.stdout);(out/(label+'.trace.stderr')).write_text(p.stderr);assert p.returncode==0
  events=[json.loads(s) for s in p.stderr.splitlines() if s.startswith('{')];results.append(dict(arm=arm,fixture=name,worker_sha256=hashlib.sha256(worker.read_bytes()).hexdigest(),fixture_sha256=hashlib.sha256((fixtures/(name+'.json')).read_bytes()).hexdigest(),trace_cycles=len(events),trace_full=sum(e['collection_kind']=='full' for e in events),trace_pointer_slots=sum(e['layout_scans']['pointer_slots_read'] for e in events)))
  cmd=[str(worker),str(fixtures/(name+'.json')),'parse','60000' if name=='unicode_1m' else '10000','2']
  with (out/(label+'.stdout')).open('w') as stdout,(out/(label+'.stderr')).open('w') as stderr:
   process=subprocess.Popen(cmd,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
   try:
    time.sleep(.5)
    sample=subprocess.run(['/usr/bin/sample',str(process.pid),'3','1','-mayDie','-file',str(out/(label+'.sample.txt'))],capture_output=True,text=True,timeout=20)
    results[-1].update(sample_exit=sample.returncode,sample_stdout=sample.stdout,sample_stderr=sample.stderr);assert sample.returncode==0,sample.stderr
   finally:
    if process.poll() is None:os.killpg(process.pid,signal.SIGTERM)
    try:process.wait(timeout=5)
    except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
    results[-1]['worker_terminated_after_sample']=process.returncode
  (out/'summary.json').write_text(json.dumps(results,indent=2)+'\n');print('PROFILED',label,results[-1]['trace_cycles'],flush=True)
