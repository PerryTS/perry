from pathlib import Path
import json, shlex, shutil, subprocess

w = Path(__file__).resolve().parent
prefix = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
cmd = ['python3', prefix + 'run-changing-options.py', '--worker', prefix + 'candidate-changing-options',
       '--baseline-worker', prefix + 'main-changing-options', '--node', '/opt/homebrew/bin/node',
       '--bun', '/Users/perry/.bun/bin/bun']
cases=json.loads((w/'changing-cases.json').read_text())['cases']
assert len(cases)==2 and all(c['repetitions']==7 for c in cases)
for c in cases:
 cmd+=['--case',':'.join(str(c[k]) for k in ['fixture','operation','iterations','warmup','repetitions'])]
slug = 'quiet-' + w.name + '-changing-options'
invocation = ['python3', 'with_lock.py', '--'] + cmd + ['--results-dir', 'results/' + slug]
(w / 'remote-changing-options-command.json').write_text(json.dumps(invocation, indent=2) + '\n')
with (w / 'remote-changing-options.log').open('wb') as log:
    result = subprocess.run(['ssh', host, 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)], stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-changing-options.log').read_text(), flush=True)
subprocess.run(['python3', str(w / 'archive-results.py'), slug, 'options', '--allow-failed-window'], check=True)
d = w.parents[1] / 'results' / slug
for name in ['remote-changing-options-command.json', 'remote-changing-options.log', 'remote-stage-hashes.json', 'changing-cases.json', 'measure-changing-options.py']:
    shutil.copy2(w / name, d / name)
(d / 'controller-exit.json').write_text(json.dumps({'exit_code': result.returncode}, indent=2) + '\n')
if result.returncode:
    raise SystemExit(result.returncode)
