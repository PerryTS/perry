from pathlib import Path
import json, shlex, subprocess

w = Path(__file__).resolve().parent
prefix = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
common = ['--node', '/opt/homebrew/bin/node', '--bun', '/Users/perry/.bun/bin/bun']
access = ['python3', prefix + 'run-access.py', '--worker', prefix + 'candidate-access-worker',
          '--baseline-worker', prefix + 'main-access-worker', '--iterations', '5000000', '--repeat', '11'] + common
options = ['python3', prefix + 'run-options.py', '--worker', prefix + 'candidate-options',
           '--baseline-worker', prefix + 'main-options'] + common
for operation in ['keys', 'plain', 'zero', 'dynamic-zero']:
    options += ['--case', 'small_record:' + operation + ':2000000:5000:11']
for kind, cmd in [('access', access), ('options', options)]:
    slug = 'quiet-' + w.name + '-recheck-' + kind
    invocation = ['python3', 'with_lock.py', '--'] + cmd + ['--results-dir', 'results/' + slug]
    (w / ('remote-recheck-' + kind + '-command.json')).write_text(json.dumps(invocation, indent=2) + '\n')
    with (w / ('remote-recheck-' + kind + '.log')).open('wb') as log:
        result = subprocess.run(['ssh', host, 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)], stdout=log, stderr=subprocess.STDOUT)
    print((w / ('remote-recheck-' + kind + '.log')).read_text(), flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
    subprocess.run(['python3', str(w / 'archive-results.py'), slug, kind], check=True)
