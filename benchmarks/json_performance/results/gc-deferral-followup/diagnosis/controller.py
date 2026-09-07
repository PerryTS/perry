from pathlib import Path
import json,subprocess,time
out=Path(__file__).resolve().parent
obs=[]
with (out/'controller.log').open('w') as log:
 p=subprocess.Popen(['ssh','-o','BatchMode=yes','perry@perry-macos.local','cd ~/json-deferred-gc-diagnosis-20260907-codex && python3 with_lock.py -- python3 diagnose.py'],stdout=log,stderr=subprocess.STDOUT)
 while p.poll() is None:
  c=subprocess.run(['ssh','-o','BatchMode=yes','perry@perry-macos.local','ps -Ao pid,ppid,pcpu,comm'],capture_output=True,text=True,timeout=20)
  ext=[l.strip() for l in c.stdout.splitlines() if any(x in l for x in ['/ccperf/','/rustc','/cargo','/worker']) and 'json-deferred-gc-v2-20260907-codex/' not in l]
  obs.append({'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'monitor_exit':c.returncode,'external':ext})
  (out/'observations.json').write_text(json.dumps(obs,indent=2)+'\n')
  time.sleep(5)
 print('FINISHED',p.returncode,flush=True)
 assert p.returncode==0
