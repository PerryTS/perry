from pathlib import Path
import argparse, json, shlex, shutil, subprocess

w = Path(__file__).resolve().parent
p = '.work/' + w.name + '/'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
parser = argparse.ArgumentParser()
parser.add_argument('phase', choices=['access', 'focus', 'options', 'rotating', 'retained'])
kind = parser.parse_args().phase
assert kind == 'rotating'
worker = {'access': 'access-worker', 'rotating': 'rotating-worker', 'options': 'options'}.get(kind, 'worker')
cmd = ['python3', p + 'run-' + kind + '.py', '--worker', p + 'candidate-' + worker,
       '--baseline-worker', p + 'main-' + worker, '--node', '/opt/homebrew/bin/node',
       '--bun', '/Users/perry/.bun/bin/bun']
if kind == 'focus':
    cases = ['records_array_16k:scan:4000:8:7', 'records_array_1m:scan:200:2:7',
             'records_array_8m:scan:32:1:7', 'records_array_20m:scan:16:1:7',
             'records_array_20m:roundtrip:8:1:7', 'records_array_1m:parse:200:8:7',
             'records_array_1m:stringify:256:8:7', 'small_record:parse:2000000:5000:7',
             'small_record:stringify:10000000:5000:7', 'long_string_1m:stringify:4096:8:7',
             'null:stringify:20000000:5000:7', 'string_a:stringify:20000000:5000:7',
             'empty_object:stringify:10000000:5000:7', 'tiny_object:stringify:10000000:5000:7',
             'object_1k:stringify:2000000:5000:7']
    for case in cases:
        cmd += ['--case', case]
elif kind == 'rotating':
    cmd += ['--filter', 'small_record',
            '--repeat', '11', '--source-commit', json.loads((w / 'provenance.json').read_text())['source_commit']]
slug = 'quiet-' + w.name + '-small-recheck-' + kind
invocation = ['python3', 'with_lock.py', '--'] + cmd + ['--results-dir', 'results/' + slug]
command_file = w / ('remote-small-recheck-' + kind + '-command.json')
log_file = w / ('remote-small-recheck-' + kind + '.log')
command_file.write_text(json.dumps(invocation, indent=2) + '\n')
with log_file.open('wb') as log:
    result = subprocess.run(['ssh', 'perry@perry-macos.local', 'cd ' + shlex.quote(remote) + ' && ' + shlex.join(invocation)],
                            stdout=log, stderr=subprocess.STDOUT)
print(log_file.read_text(), flush=True)
# Preserve every terminal window before any other remote operation, including failures.
subprocess.run(['python3', str(w / 'archive-results.py'), slug, kind, '--allow-failed-window'], check=True)
d = w.parents[1] / 'results' / slug
for source in [command_file, log_file, w / 'remote-stage-hashes.json', Path(__file__)]:
    shutil.copy2(source, d / source.name)
(d / 'controller-exit.json').write_text(json.dumps({'exit_code': result.returncode}, indent=2) + '\n')
raise SystemExit(result.returncode)
