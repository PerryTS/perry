from pathlib import Path
import json, shlex, subprocess

w = Path(__file__).resolve().parent
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
prefix = '.work/' + w.name + '/'
slug = 'quiet-' + w.name + '-retry1-rotating'
cmd = ['python3', 'with_lock.py', '--', 'python3', prefix + 'run-rotating.py',
       '--worker', prefix + 'candidate-rotating-worker',
       '--baseline-worker', prefix + 'main-rotating-worker',
       '--node', '/opt/homebrew/bin/node', '--bun', '/Users/perry/.bun/bin/bun',
       '--filter', 'unicode_1m,long_string_1m,escaped_1m,small_record,records_array_1m',
       '--repeat', '7', '--source-commit', json.loads((w / 'provenance.json').read_text())['source_commit'],
       '--results-dir', 'results/' + slug]
(w / 'remote-retry1-rotating-command.json').write_text(json.dumps(cmd, indent=2) + '\n')
with (w / 'remote-retry1-rotating.log').open('wb') as log:
    result = subprocess.run(['ssh', 'perry@perry-macos.local',
                             'cd ' + shlex.quote(remote) + ' && ' + shlex.join(cmd)],
                            stdout=log, stderr=subprocess.STDOUT)
print((w / 'remote-retry1-rotating.log').read_text(), flush=True)
if result.returncode:
    raise SystemExit(result.returncode)
subprocess.run(['python3', str(w / 'archive-results.py'), slug, 'rotating'], check=True)
