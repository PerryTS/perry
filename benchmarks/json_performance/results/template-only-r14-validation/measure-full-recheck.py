from pathlib import Path
import json, shlex, subprocess

w = Path(__file__).resolve().parent
prefix = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
cmd = ['python3', prefix + 'run-focus.py', '--worker', prefix + 'candidate-worker',
       '--baseline-worker', prefix + 'main-worker', '--node', '/opt/homebrew/bin/node',
       '--bun', '/Users/perry/.bun/bin/bun']
cases = json.loads((w / 'full-recheck-cases.json').read_text())['cases']
assert len(cases) == 7 and all(c['repetitions'] == 11 for c in cases)
for c in cases:
    cmd += ['--case', ':'.join(str(c[k]) for k in ['fixture', 'operation', 'iterations', 'warmup', 'repetitions'])]
slug = 'quiet-' + w.name + '-recheck-full'
invocation = ['python3', 'with_lock.py', '--'] + cmd + ['--results-dir', 'results/' + slug]
(w / 'remote-recheck-full-command.json').write_text(json.dumps(invocation, indent=2) + '\n')
with (w / 'remote-recheck-full.log').open('wb') as log:
    result = subprocess.run(['ssh', host, 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)], stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-recheck-full.log').read_text(), flush=True)
if result.returncode:
    raise SystemExit(result.returncode)
subprocess.run(['python3', str(w / 'archive-results.py'), slug, 'full'], check=True)
