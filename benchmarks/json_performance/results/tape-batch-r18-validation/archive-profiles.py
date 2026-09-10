"""Archive the terminal profile window before any subsequent remote operation."""
from pathlib import Path
import shlex
import subprocess

w = Path(__file__).resolve().parent
bench = w.parents[1]
slug = 'quiet-' + w.name + '-profiles'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
code = '''from pathlib import Path
import json,shutil
root=Path(REMOTE);w=root/'.work'/WORK;d=root/'results'/SLUG
window=json.loads((root/'results/window-custom.json').read_text())
assert window.get('finished_utc') and 'results/'+SLUG in window['command']
d.mkdir(parents=True,exist_ok=True)
for name,target in [('results/window-custom.json','window.json'),('custom.log','controller.log'),('results/processes-before.txt','processes-before.txt'),('results/processes-after.txt','processes-after.txt'),('with_lock.py','with_lock.py'),('run_dispatch_focus.py','run_dispatch_focus.py')]:shutil.copy2(root/name,d/target)
for name in ['run-profiles.py','profile-cases.json','provenance.json','source.patch','main-build-provenance.json','build-provenance.json','main-workers-provenance.json','candidate-workers-provenance.json','root-comparison.json','candidate-fixture-validation.json','candidate-options-validation.json','fixture-hashes.json']:shutil.copy2(w/name,d/name)
shutil.copytree(w/'harness',d/'harness',dirs_exist_ok=True)
print('TERMINAL_WINDOW',window['started_utc'],window['finished_utc'],'quiet_gate',window['quiet_gate_passed'])
'''.replace('REMOTE', repr(remote)).replace('WORK', repr(w.name)).replace('SLUG', repr(slug))
subprocess.run(['ssh', host, 'python3 -c ' + shlex.quote(code)], check=True)
subprocess.run(['rsync', '-a', host + ':' + remote + '/results/' + slug + '/', str(bench / 'results' / slug) + '/'], check=True)
print('ARCHIVED', slug, flush=True)
