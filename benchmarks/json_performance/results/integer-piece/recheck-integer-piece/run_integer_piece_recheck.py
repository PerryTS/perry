from pathlib import Path
import subprocess,json,time
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');observations=[]
with (art/'integer-piece-recheck-window.log').open('w') as log:
 p=subprocess.Popen(['ssh','-o','BatchMode=yes','perry@perry-macos.local','cd ~/json-integer-piece-20260906-codex && python3 with_lock.py -- python3 recheck_integer_piece.py'],stdout=log,stderr=subprocess.STDOUT)
 print('SSH_PID',p.pid,flush=True)
 while p.poll() is None:
  check=subprocess.run(['ssh','-o','BatchMode=yes','perry@perry-macos.local','ps -Ao pid,ppid,pcpu,comm'],capture_output=True,text=True,timeout=20)
  external=[line.strip() for line in check.stdout.splitlines() if any(x in line for x in ['/ccperf/','/rustc','/cargo','/worker']) and 'json-integer-piece-20260906-codex/' not in line]
  observations.append({'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'monitor_exit':check.returncode,'external':external})
  (art/'integer-piece-recheck-external-observations.json').write_text(json.dumps(observations,indent=2)+'\n')
  time.sleep(5)
 print('FINISHED',p.returncode,flush=True)
 assert p.returncode==0
