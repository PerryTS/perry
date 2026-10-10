from pathlib import Path
import os, subprocess, time
L = Path('/root/lanes/perry-unpackcpu')
while not (L / 'profile-final-node.rc').exists():
    time.sleep(15)
assert (L / 'profile-final-node.rc').read_text().strip() == '0'
programs = ['hello', 'tsc', 'zod', 'qs_parse', 'qs_stringify', 'commander', 'fastify', 'effect', 'buffer_heavy', 'worker_heavy']

def run(tag, cmd, env):
    with (L / (tag + '.log')).open('w') as f:
        result = subprocess.run(['taskset', '-c', '0-55', *cmd], stdout=f, stderr=f, env=env)
    (L / (tag + '.rc')).write_text(str(result.returncode))
    print(tag, result.returncode, flush=True)
    assert result.returncode == 0

noise = os.environ.copy()
noise['ARMS'] = 'base,repeat'
run('programs-noise', ['python3', str(L / 'measure.py'), 'programs-noise', *programs], noise)
run('upm-noise', ['python3', str(L / 'measure.py'), 'upm-noise', 'cold', 'lock', 'offinst'], noise)
run('micro-noise', ['python3', str(L / 'micro_measure.py'), 'noise', 'hash', 'inflate', 'worker', 'worker-stream'], noise)
env = os.environ.copy()
env.update(ARMS='base,fix', THP_OFF='1')
for kind in ['programs-thpoff', 'programs-thpoff-gc']:
    run(kind, ['python3', str(L / 'measure.py'), kind, *programs], env)
for kind in ['upm-thpoff', 'upm-thpoff-gc']:
    run(kind, ['python3', str(L / 'measure.py'), kind, 'cold', 'lock'], env)
for kind in ['thpoff', 'thpoff-gc']:
    run('micro-' + kind, ['python3', str(L / 'micro_measure.py'), kind, 'hash', 'inflate', 'worker', 'worker-stream'], env)
print('COMPLETE', flush=True)
