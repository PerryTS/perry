from pathlib import Path
import hashlib, json, shlex, subprocess, sys

w = Path(__file__).resolve().parent
bench = w.parents[1]
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
slug = 'quiet-' + w.name + '-aa-main-full'
expected = hashlib.sha256((w / 'main-worker').read_bytes()).hexdigest()
code = '''from pathlib import Path
import hashlib,json,shutil
root=Path(REMOTE);w=root/'.work'/WORKNAME;d=root/'results'/SLUG
window=json.loads((root/'results/window-custom.json').read_text())
assert window['quiet_gate_passed'] and window.get('finished_utc') and 'results/'+SLUG in window['command']
command=window['command'];assert command[command.index('--worker')+1]==command[command.index('--baseline-worker')+1]
host=json.loads((d/'host.json').read_text());assert host['workers']['perry']==host['workers']['baseline']==EXPECTED
assert hashlib.sha256((w/'main-worker').read_bytes()).hexdigest()==EXPECTED
for source,name in [(root/'results/window-custom.json','window.json'),(root/'custom.log','controller.log')]:shutil.copy2(source,d/name)
for name in ['processes-before.txt','processes-after.txt']:shutil.copy2(root/'results'/name,d/name)
for name in ['run_dispatch_focus.py','with_lock.py']:shutil.copy2(root/name,d/name)
for name in ['main-build-provenance.json','main-workers-provenance.json','fixture-hashes.json','run-focus.py']:shutil.copy2(w/name,d/name)
shutil.copytree(w/'harness',d/'harness',dirs_exist_ok=True)
base=json.loads((w/'main-build-provenance.json').read_text())['source_commit']
(d/'provenance.json').write_text(json.dumps({'experiment':'A/A timing-order control','source_commit':base,'both_perry_arms':'same frozen main executable path and bytes','worker_sha256':EXPECTED,'candidate_measured':False},indent=2)+'\\n')
print(window['started_utc'],window['finished_utc'])
'''.replace('REMOTE', repr(remote)).replace('WORKNAME', repr(w.name)).replace('SLUG', repr(slug)).replace('EXPECTED', repr(expected))
subprocess.run(['ssh', 'perry@perry-macos.local', 'python3 -c ' + shlex.quote(code)], check=True)
subprocess.run(['rsync', '-a', 'perry@perry-macos.local:' + remote + '/results/' + slug + '/',
                str(bench / 'results' / slug) + '/'], check=True)
print('ARCHIVED A/A control; no candidate patch or build provenance attributed to this measurement.', flush=True)
