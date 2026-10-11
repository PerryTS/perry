#!/usr/bin/env python3
"""Count GC phases using the selected library graph, outside acceptance runs."""
import argparse
import concurrent.futures
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
p = argparse.ArgumentParser()
p.add_argument('--name', required=True)
p.add_argument('--only', nargs='+', required=True)
a = p.parse_args()


def build(arm):
    base = ROOT / ('phase-' + arm)
    original = ROOT / ('obs-' + arm)
    base.mkdir(exist_ok=True)
    subprocess.run(['rsync', '-a', '--delete', '--exclude', 'target',
                    str(original / 'src') + '/', str(base / 'src') + '/'], check=True)
    # This diagnostic intentionally uses the already selected Fastify graph.
    # Each arm is seeded only from its own observer build.
    target = base / 'target'
    if target.exists():
        shutil.rmtree(target)
    seed = original / 'src/target/perry-no-auto-stream-dispatch'
    subprocess.run(['cp', '-a', '--reflink=auto', str(seed), str(target)], check=True)
    selected = base / 'src/target/perry-no-auto-stream-dispatch'
    if selected.is_symlink():
        selected.unlink()
    elif selected.exists():
        shutil.rmtree(selected)
    selected.parent.mkdir(exist_ok=True)
    selected.symlink_to(target)
    subprocess.run(['python3', str(ROOT / 'src/lane-keylists/prepare-phases.py'),
                    str(base / 'src')], check=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target),
               PERRY_RUNTIME_DIR=str(target / 'release'),
               PERRY_WORKSPACE_ROOT=str(base / 'src'), CARGO_BUILD_JOBS='1')
    with (ROOT / ('keylists2-' + a.name + '-' + arm + '-build.log')).open('w') as out:
        subprocess.run(['taskset', '-c', '0-23,32-55', 'cargo', 'build',
                        '--release', '-j1', '-p', 'perry-runtime',
                        '-p', 'perry-runtime-static', '-p', 'perry-stdlib-static',
                        '--features', 'perry-stdlib/external-http-client-pump',
                        '-p', 'perry-ext-http'], cwd=base / 'src', env=env,
                       stdout=out, stderr=subprocess.STDOUT, check=True)


with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    list(pool.map(build, ['main', 'head']))
env = dict(os.environ, KEYLISTS_MAIN='main-current', KEYLISTS_EVIDENCE='evidence2',
           KEYLISTS_RECORD_NAME='phases-' + a.name + '.json')
harness = ROOT / 'src/lane-keylists'
with (ROOT / ('keylists2-' + a.name + '.log')).open('w') as out:
    for arm in ['phase-main', 'phase-head']:
        subprocess.run(['python3', 'relink.py', arm, '--only', *a.only],
                       cwd=harness, env=env, stdout=out, stderr=subprocess.STDOUT,
                       check=True)
    subprocess.run(['python3', 'run.py', 'diag', 'phase-main', 'phase-head',
                    '--n', '3', '--only', *a.only], cwd=harness, env=env,
                   stdout=out, stderr=subprocess.STDOUT, check=True)
