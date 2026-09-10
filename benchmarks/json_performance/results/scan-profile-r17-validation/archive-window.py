"""Preserve every terminal diagnostic window, including failed/partial runs.
Qualification is a separate analyzer check; archiving never marks a run valid.
"""
from pathlib import Path
import json,shlex,subprocess,sys
w=Path(__file__).resolve().parent;bench=w.parents[1];slug=sys.argv[1];assert slug.startswith('quiet-'+w.name+'-')
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';host='perry@perry-macos.local'
code='''from pathlib import Path
from datetime import datetime
import hashlib,json,shutil
root=Path(REMOTE);w=root/'.work'/WORK;d=root/'results'/SLUG
a=root/'results/window-custom.json';window=json.loads(a.read_text());assert window.get('finished_utc') and 'results/'+SLUG in window['command']
d.mkdir(parents=True,exist_ok=True);shutil.copy2(a,d/'window.json')
start=datetime.fromisoformat(window['started_utc'].replace('Z','+00:00')).timestamp()
log=root/'custom.log'
if log.exists() and log.stat().st_mtime>=start:shutil.copy2(log,d/'controller.log')
for key,name in [('competing_workloads_before','processes-before.txt'),('competing_workloads_after','processes-after.txt')]:
 if key in window:shutil.copy2(root/'results'/name,d/name)
for name in ['with_lock.py','run_dispatch_focus.py']:shutil.copy2(root/name,d/name)
for name in ['run-modes.py','mode-cases.json','main-workers-provenance.json','main-build-provenance.json','reference-main.json','validation-reference.json','fixture-hashes.json']:shutil.copy2(w/name,d/name)
for name in ['run-profiles.py','profile-cases.json','run-memory.py','memory-cases.json','memory-worker.ts','memory-worker-provenance.json']:
 if (w/name).exists():shutil.copy2(w/name,d/name)
shutil.copytree(w/'harness',d/'harness',dirs_exist_ok=True)
print('TERMINAL_WINDOW',window['started_utc'],window['finished_utc'],'quiet_gate',window['quiet_gate_passed'])
'''.replace('REMOTE',repr(remote)).replace('WORK',repr(w.name)).replace('SLUG',repr(slug))
subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
subprocess.run(['rsync','-a',host+':'+remote+'/results/'+slug+'/',str(bench/'results'/slug)+'/'],check=True)
print('ARCHIVED',slug,flush=True)
