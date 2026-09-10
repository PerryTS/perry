from pathlib import Path
import json, shlex, subprocess, sys
base=Path(__file__).resolve().parent.parent
slug, candidate, kind=sys.argv[1:]
assert all(x and all(c.isalnum() or c=='-' for c in x) for x in [slug,candidate])
assert kind in ['rotating','original','focus']
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
drivers={'rotating':['run_rotating.py','with_lock.py','rotating-worker.js','rotating-worker.ts'], 'original':['run_baseline.py','with_lock.py','worker.js','worker.ts'], 'focus':['run_regression_focus.py','run_dispatch_focus.py','with_lock.py','worker.js','worker.ts']}[kind]
code=f'''from pathlib import Path
import json,shutil
root=Path({remote!r})
dest=root/'results'/{slug!r}
w=json.loads((root/'results/window-custom.json').read_text())
assert w['quiet_gate_passed'] and w.get('finished_utc')
assert 'results/'+{slug!r} in w['command']
if (dest/'window.json').exists():assert json.loads((dest/'window.json').read_text())==w
for src,name in [(root/'results/window-custom.json','window.json'),(root/'custom.log','controller.log')]:shutil.copy2(src,dest/name)
for name in ['processes-before.txt','processes-after.txt']:shutil.copy2(root/'results'/name,dest/name)
for name in {drivers!r}:shutil.copy2(root/name,dest/name)
for name in ['provenance.json','source.patch','gc-witness.json','codegen-comparison.json']:
 shutil.copy2(root/'.work'/{candidate!r}/name,dest/name)
if '--baseline-worker' in w['command']:
 reference=(root/w['command'][w['command'].index('--baseline-worker')+1]).parent
 for name in ['provenance.json','gc-witness.json','entry-codegen.json']:
  if (reference/name).exists():shutil.copy2(reference/name,dest/('reference-'+name))
print(w['started_utc'],w['finished_utc'])
'''
subprocess.run(['ssh','perry@perry-macos.local','python3 -c '+shlex.quote(code)],check=True)
subprocess.run(['rsync','-a','perry@perry-macos.local:'+remote+'/results/'+slug+'/',str(base/'results'/slug)+'/'],check=True)
