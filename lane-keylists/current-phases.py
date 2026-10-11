#!/usr/bin/env python3
"""Attribute the current-base residual with private, independent targets."""
import concurrent.futures, os, subprocess, time
from pathlib import Path
R=Path('/root/codex-lanes/cx-keylists')
for arm in ('head','obs-head'):
    log=R/f'keylists2-current72-simple-{arm}-build.log'
    while 'Finished ' not in log.read_text():
        if 'could not compile' in log.read_text(): raise RuntimeError(log)
        time.sleep(10)

def build(arm):
    base=R/('phase-'+arm); original=R/('obs-'+arm)
    base.mkdir(exist_ok=True)
    subprocess.run(['rsync','-a','--delete','--exclude','target',str(original/'src')+'/',str(base/'src')+'/'],check=True)
    if not (base/'target').exists():
        subprocess.run(['cp','-a','--reflink=auto',str(original/'target'),str(base/'target')],check=True)
    subprocess.run(['python3',str(R/'src/lane-keylists/prepare-phases.py'),str(base/'src')],check=True)
    env=dict(os.environ,CARGO_TARGET_DIR=str(base/'target'),PERRY_RUNTIME_DIR=str(base/'target/release'),PERRY_WORKSPACE_ROOT=str(base/'src'))
    with (R/f'keylists2-current72-phase-{arm}-build.log').open('w') as out:
        subprocess.run(['taskset','-c','0-23,32-55','cargo','build','--release','-j3','-p','perry-runtime','-p','perry-runtime-static','-p','perry-stdlib-static'],cwd=base/'src',env=env,stdout=out,stderr=subprocess.STDOUT,check=True)
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as p:
    list(p.map(build,('main','head')))
env=dict(os.environ,KEYLISTS_MAIN='main-current',KEYLISTS_EVIDENCE='evidence2',KEYLISTS_RECORD_NAME='phases-current72-simple.json')
h=R/'src/lane-keylists'; only=['--only','root-survival','buffer_heavy','worker_heavy']
with (R/'keylists2-current72-phases.log').open('w') as out:
    for arm in ('phase-main','phase-head'):
        subprocess.run(['python3','relink.py',arm,*only],cwd=h,env=env,stdout=out,stderr=subprocess.STDOUT,check=True)
    subprocess.run(['python3','run.py','diag','phase-main','phase-head','--n','3',*only],cwd=h,env=env,stdout=out,stderr=subprocess.STDOUT,check=True)
