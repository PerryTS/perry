from pathlib import Path
import json, shlex, subprocess, sys
base=Path(__file__).resolve().parents[2]
slug, candidate, kind=sys.argv[1:]
assert all(x and all(c.isalnum() or c=='-' for c in x) for x in [slug,candidate])
assert kind in ['rotating','original','focus','access','short','options']
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
drivers={'options':['run_dispatch_focus.py','with_lock.py'], 'short':['run_dispatch_focus.py','with_lock.py'], 'rotating':['run_rotating.py','with_lock.py','rotating-worker.js','rotating-worker.ts'], 'original':['run_baseline.py','with_lock.py','worker.js','worker.ts'], 'access':['run_access.py','run_dispatch_focus.py','with_lock.py','access-worker.js','access-worker.ts'], 'focus':['run_regression_focus.py','run_dispatch_focus.py','with_lock.py','worker.js','worker.ts']}[kind]
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
if {kind!r} == 'options':
 for name in ['run-options.py','options-worker.ts','options-worker.js','candidate-options-provenance.json','reference-options-provenance.json','candidate-options-validation.json','reference-options-validation.json']:
  shutil.copy2(root/'.work'/{candidate!r}/name,dest/name)
if {kind!r} == 'access':
 shutil.copy2(root/'.work'/{candidate!r}/'run-access.py',dest/'run-access.py')
if {kind!r} != 'options' and '--prior-worker' in w['command']:
 reference=(root/w['command'][w['command'].index('--prior-worker')+1]).parent
 for name in ['provenance.json','build-provenance.json']:
  if (reference/name).exists():shutil.copy2(reference/name,dest/('prior-'+name))
if {kind!r} == 'short':
 for name in ['short-loops.ts','short-loops.js','run-short-loops.py','short-reference-provenance.json','short-provenance.json']:
  shutil.copy2(root/'.work'/{candidate!r}/name,dest/name)
if {kind!r} not in ['short','options'] and '--baseline-worker' in w['command']:
 reference=(root/w['command'][w['command'].index('--baseline-worker')+1]).parent
 for name in ['provenance.json','main-access-provenance.json','gc-witness.json','entry-codegen.json']:
  if (reference/name).exists():shutil.copy2(reference/name,dest/('reference-'+name))
print(w['started_utc'],w['finished_utc'])
'''
subprocess.run(['ssh','perry@perry-macos.local','python3 -c '+shlex.quote(code)],check=True)
subprocess.run(['rsync','-a','perry@perry-macos.local:'+remote+'/results/'+slug+'/',str(base/'results'/slug)+'/'],check=True)
