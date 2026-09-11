from pathlib import Path
import gzip, hashlib, json, os, shlex, shutil, subprocess

w = Path(__file__).resolve().parent
bench = w.parents[1]
root = w.parents[3]
host = 'perry@perry-macos.local'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
prefix = '.work/' + w.name + '/'
control = json.loads((w / 'r22-shared-worker-provenance.json').read_text())
for path, digest in control['files'].items():
    assert hashlib.sha256((root / path).read_bytes()).hexdigest() == digest
assert (w / 'r22-shared-worker.o').read_bytes() == (w / 'main-worker.o').read_bytes() == (w / 'candidate-worker.o').read_bytes()
expected = json.loads((w / 'remote-stage-hashes.json').read_text())
new_files = [prefix + name for name in ['r22-shared-worker', 'r22-shared-worker-provenance.json', 'r22-control-source.patch']]
for path in new_files:
    expected[path] = hashlib.sha256((bench / path).read_bytes()).hexdigest()

def ssh(code):
    subprocess.run(['ssh', host, 'python3 -c ' + shlex.quote(code)], check=True)

token = 'json-r23-r22-control-stage-' + str(os.getpid())
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text(' + repr(token) + ')')
try:
    subprocess.run(['rsync', '-aR', *new_files, host + ':' + remote + '/'], cwd=bench, check=True)
    ssh('from pathlib import Path;import hashlib;r=Path(' + repr(remote) + ');e=' + repr(expected) + ';assert all(hashlib.sha256((r/p).read_bytes()).hexdigest()==h for p,h in e.items());print("Verified control inputs",len(e))')
finally:
    ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()==' + repr(token) + ';(p/"owner").unlink();p.rmdir()')
(w / 'r22-control-stage-hashes.json').write_text(json.dumps(expected, indent=2) + '\n')

cases = json.loads((w / 'regression-cases.json').read_text())['cases']
assert len(cases) == 5 and all(c['repetitions'] == 11 for c in cases)
slug = 'quiet-' + w.name + '-r22-control-focus'
cmd = ['python3', prefix + 'run-focus.py', '--worker', prefix + 'r22-shared-worker', '--baseline-worker', prefix + 'main-worker', '--node', '/opt/homebrew/bin/node', '--bun', '/Users/perry/.bun/bin/bun']
for c in cases:
    cmd += ['--case', ':'.join(str(c[k]) for k in ['fixture', 'operation', 'iterations', 'warmup', 'repetitions'])]
invocation = ['python3', 'with_lock.py', '--', *cmd, '--results-dir', 'results/' + slug]
(w / 'remote-r22-control-command.json').write_text(json.dumps(invocation, indent=2) + '\n')
with (w / 'remote-r22-control.log').open('wb') as log:
    result = subprocess.run(['ssh', host, 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)], stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-r22-control.log').read_text(), flush=True)

# The FIRST remote operation after the terminal window archives that window.
code = '''from pathlib import Path
import json,shutil
r=Path(REMOTE);w=r/'.work'/WORKNAME;d=r/'results'/SLUG
window=json.loads((r/'results/window-custom.json').read_text())
assert window.get('finished_utc') and 'results/'+SLUG in window['command']
d.mkdir(parents=True,exist_ok=True)
for src,name in [(r/'results/window-custom.json','window.json'),(r/'custom.log','controller.log')]:shutil.copy2(src,d/name)
for name in ['processes-before.txt','processes-after.txt','fixtures.json']:shutil.copy2(r/'results'/name,d/name)
for name in ['run_dispatch_focus.py','with_lock.py']:shutil.copy2(r/name,d/name)
for name in ['run-focus.py','fixture-hashes.json','r22-shared-worker-provenance.json','r22-control-source.patch']:shutil.copy2(w/name,d/name)
shutil.copytree(w/'harness',d/'harness',dirs_exist_ok=True)
print(window['started_utc'],window['finished_utc'])
'''.replace('REMOTE', repr(remote)).replace('WORKNAME', repr(w.name)).replace('SLUG', repr(slug))
ssh(code)
d = bench / 'results' / slug
subprocess.run(['rsync', '-a', host + ':' + remote + '/results/' + slug + '/', str(d) + '/'], check=True)
for name in ['remote-r22-control-command.json', 'remote-r22-control.log', 'r22-control-stage-hashes.json', 'regression-cases.json', 'main-build-provenance.json', 'main-workers-provenance.json']:
    shutil.copy2(w / name, d / name)
shutil.copy2(Path(__file__), d / Path(__file__).name)
raw = (d / 'r22-control-source.patch').read_bytes()
data = gzip.compress(raw, compresslevel=9, mtime=0)
assert gzip.decompress(data) == raw
(d / 'source.patch.gz').write_bytes(data)
(d / 'source.patch.json').write_text(json.dumps({'original_bytes': len(raw), 'original_sha256': hashlib.sha256(raw).hexdigest(), 'gzip_sha256': hashlib.sha256(data).hexdigest()}, indent=2) + '\n')
(d / 'r22-control-source.patch').unlink()
(d / 'controller-exit.json').write_text(json.dumps({'exit_code': result.returncode}, indent=2) + '\n')
(d / 'comparison-roles.json').write_text(json.dumps({'purpose': 'Isolate R22 sparse-cache change using benchmark object code identical to R23 comparison', 'perry_engine': control, 'baseline_engine': json.loads((w / 'main-build-provenance.json').read_text()), 'not_the_r23_candidate': True}, indent=2) + '\n')
print('ARCHIVED', slug, flush=True)
raise SystemExit(result.returncode)
