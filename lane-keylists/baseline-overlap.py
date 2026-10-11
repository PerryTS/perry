#!/usr/bin/env python3
"""Prepare a fresh baseline and overlap its large tsc frontend with libraries."""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(os.environ['KEYLISTS_ROOT'])
NAME = os.environ['KEYLISTS_ROUND_NAME']
ENV = dict(os.environ, KEYLISTS_MAIN='main-current', KEYLISTS_EVIDENCE='evidence2')
HARNESS = ROOT / 'src/lane-keylists'


def run(argv, log, *, base=None, jobs=2, required=True):
    base = base or ROOT
    env = dict(ENV, CARGO_TARGET_DIR=str(base/'target'),
               PERRY_RUNTIME_DIR=str(base/'target/release'),
               PERRY_WORKSPACE_ROOT=str(base/'src'), CARGO_BUILD_JOBS=str(jobs))
    with (ROOT/f'keylists2-{NAME}-{log}.log').open('w') as out:
        result = subprocess.run(['taskset', '-c', '0-23,32-55', *argv],
                                cwd=base/'src' if argv[0]=='cargo' else HARNESS,
                                env=env, stdout=out, stderr=subprocess.STDOUT)
    if required:
        assert result.returncode == 0, (argv, result.returncode, log)


def parallel(*jobs):
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(jobs)) as pool:
        for result in [pool.submit(job) for job in jobs]:
            result.result()


main = ROOT/'main-current'
run(['cargo', 'build', '--release', '-j2', '-p', 'perry', '-p', 'perry-runtime',
     '-p', 'perry-runtime-static', '-p', 'perry-stdlib-static'], 'main-build', base=main)
refresh=['--refresh'] if (ROOT/'obs-main/target').is_dir() else []
run(['python3', 'prepare-observation.py', 'main', *refresh], 'prepare-main')
tsc_log = (ROOT/f'keylists2-{NAME}-early-tsc.log').open('w')
tsc = subprocess.Popen(['taskset', '-c', '0-23,32-55', 'python3',
                        str(HARNESS/'tsc-overlap.py')], cwd=HARNESS, env=ENV,
                       stdout=tsc_log, stderr=subprocess.STDOUT)

parallel(
    lambda: run(['cargo', 'build', '--release', '-j2', '-p', 'perry-runtime',
                 '-p', 'perry-runtime-static', '-p', 'perry-stdlib-static'],
                'obs-main-build', base=ROOT/'obs-main'),
    lambda: run(['python3', 'build-stream-graph.py', 'main'], 'main-stream'),
)
parallel(
    lambda: run(['python3', 'build-stream-graph.py', 'obs-main'], 'obs-main-stream'),
    lambda: run(['cargo', 'test', '--release', '-j2', '-p', 'perry-runtime',
                 '--lib', '--', '--test-threads=1'], 'main-units', base=main,
                required=False),
)
assert 'test result:' in (ROOT/f'keylists2-{NAME}-main-units.log').read_text()
run(['python3', 'run.py', 'compile', 'main'], 'frontend')
assert tsc.wait() == 0
tsc_log.close()
statuses = json.loads((ROOT/'evidence2/main/status.json').read_text())
assert len(statuses) == 18
assert all(r['equal'] and r['compile_rc'] == r['node_rc'] == r['run_rc'] == 0
           for r in statuses.values())
(ROOT/'evidence2/baseline-ready.json').write_text(json.dumps({
    'complete': True, 'round': NAME, 'programs': len(statuses)})+'\n')
