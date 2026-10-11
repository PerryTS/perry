#!/usr/bin/env python3
"""Build the selected no-auto Fastify library graph in each independent arm."""
import argparse
import os
from pathlib import Path
import subprocess
import run as harness

p = argparse.ArgumentParser()
p.add_argument('arm', choices=['main', 'head', 'obs-main', 'obs-head'])
a = p.parse_args()
base, env = harness.arm_env(a.arm)
target = base / 'src/target/perry-no-auto-stream-dispatch'
if not target.exists():
    target.parent.mkdir(parents=True, exist_ok=True)
    # Cache cloning is confined to the same arm. Feature unification and the
    # observer source change still rebuild that arm's selected libraries.
    original = harness.ROOT if a.arm == 'obs-head' else harness.MAIN if a.arm == 'obs-main' else base
    seed = original / 'src/target/perry-no-auto-stream-dispatch'
    if not seed.exists():
        seed = base / 'target'
    subprocess.run(['cp', '-a', '--reflink=auto', str(seed), str(target)], check=True)
env.update(CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target / 'release'))
if (harness.ROOT/'tmp/early-tsc.running').exists():
    env['CARGO_BUILD_JOBS']=str(min(2,int(env['CARGO_BUILD_JOBS'])))
if (harness.ROOT/'tmp/native-current.running').exists():
    env['CARGO_BUILD_JOBS']=str(min(1,int(env['CARGO_BUILD_JOBS'])))
subprocess.run([
    'taskset', '-c', harness.CPUS, 'cargo', 'build', '--release',
    '-j', env['CARGO_BUILD_JOBS'], '-p', 'perry-runtime', '-p', 'perry-runtime-static',
    '-p', 'perry-stdlib-static', '--features', 'perry-stdlib/external-http-client-pump',
    '-p', 'perry-ext-http',
], cwd=base / 'src', env=env, check=True)
