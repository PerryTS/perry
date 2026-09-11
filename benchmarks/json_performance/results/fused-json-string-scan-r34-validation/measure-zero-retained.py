from pathlib import Path
import json, shlex, shutil, subprocess

w = Path(__file__).resolve().parent
prefix = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
cmd = ['python3', prefix + 'run-zero-retained.py', '--worker', prefix + 'candidate-retained-zero',
       '--baseline-worker', prefix + 'main-retained-zero', '--node', '/opt/homebrew/bin/node',
       '--bun', '/Users/perry/.bun/bin/bun']
slug = 'quiet-' + w.name + '-zero-retained'
invocation = ['python3', 'with_lock.py', '--'] + cmd + ['--results-dir', 'results/' + slug]
(w / 'remote-zero-retained-command.json').write_text(json.dumps(invocation, indent=2) + '\n')
with (w / 'remote-zero-retained.log').open('wb') as log:
    result = subprocess.run(['ssh', host, 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)], stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-zero-retained.log').read_text(), flush=True)
subprocess.run(['python3', str(w / 'archive-results.py'), slug, 'retained', '--allow-failed-window'], check=True)
d = w.parents[1] / 'results' / slug
for name in ['remote-zero-retained-command.json', 'remote-zero-retained.log', 'remote-stage-hashes.json', 'measure-zero-retained.py']:
    shutil.copy2(w / name, d / name)
(d / 'controller-exit.json').write_text(json.dumps({'exit_code': result.returncode}, indent=2) + '\n')
if result.returncode:
    raise SystemExit(result.returncode)
