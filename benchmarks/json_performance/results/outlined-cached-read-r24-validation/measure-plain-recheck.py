from pathlib import Path
import json, shlex, shutil, subprocess

w = Path(__file__).resolve().parent
prefix = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
cmd = ['python3', prefix + 'run-options.py', '--worker', prefix + 'candidate-options',
       '--baseline-worker', prefix + 'main-options', '--node', '/opt/homebrew/bin/node',
       '--bun', '/Users/perry/.bun/bin/bun']
cases = json.loads((w / 'plain-recheck-cases.json').read_text())['cases']
assert len(cases) == 1 and all(c['repetitions'] == 11 for c in cases)
for c in cases:
    cmd += ['--case', ':'.join(str(c[k]) for k in ['fixture', 'operation', 'iterations', 'warmup', 'repetitions'])]
slug = 'quiet-' + w.name + '-plain-recheck-options'
invocation = ['python3', 'with_lock.py', '--'] + cmd + ['--results-dir', 'results/' + slug]
(w / 'remote-plain-recheck-options-command.json').write_text(json.dumps(invocation, indent=2) + '\n')
with (w / 'remote-plain-recheck-options.log').open('wb') as log:
    result = subprocess.run(['ssh', host, 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)], stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-plain-recheck-options.log').read_text(), flush=True)
subprocess.run(['python3', str(w / 'archive-results.py'), slug, 'options', '--allow-failed-window'], check=True)
d = w.parents[1] / 'results' / slug
for name in ['remote-plain-recheck-options-command.json', 'remote-plain-recheck-options.log', 'remote-stage-hashes.json', 'plain-recheck-cases.json']:
    shutil.copy2(w / name, d / name)
(d / 'controller-exit.json').write_text(json.dumps({'exit_code': result.returncode}, indent=2) + '\n')
if result.returncode:
    raise SystemExit(result.returncode)
