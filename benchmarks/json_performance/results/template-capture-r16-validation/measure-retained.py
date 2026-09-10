from pathlib import Path
import json, shlex, subprocess

w = Path(__file__).resolve().parent
prefix = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
slug = 'quiet-' + w.name + '-retained'
cmd = ['python3', 'with_lock.py', '--', 'python3', prefix + 'run-retained.py',
       '--worker', prefix + 'candidate-worker', '--baseline-worker', prefix + 'main-worker',
       '--node', '/opt/homebrew/bin/node', '--bun', '/Users/perry/.bun/bin/bun',
       '--results-dir', 'results/' + slug]
(w / 'remote-retained-command.json').write_text(json.dumps(cmd, indent=2) + '\n')
with (w / 'remote-retained.log').open('wb') as log:
    result = subprocess.run(['ssh', 'perry@perry-macos.local',
                             'cd ' + shlex.quote(remote) + ' && ' + shlex.join(cmd)],
                            stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-retained.log').read_text(), flush=True)
if result.returncode:
    raise SystemExit(result.returncode)
subprocess.run(['python3', str(w / 'archive-results.py'), slug, 'retained'], check=True)
