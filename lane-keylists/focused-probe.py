#!/usr/bin/env python3
"""Measure completed fresh programs while the rest of a cohort compiles."""
import argparse
import datetime
import os
from pathlib import Path
import subprocess

p=argparse.ArgumentParser()
p.add_argument('--name',required=True)
p.add_argument('--only',nargs='+',required=True)
p.add_argument('--pause-child',action='store_true')
a=p.parse_args()
root=Path(os.environ['KEYLISTS_ROOT'])
env=dict(os.environ,KEYLISTS_MAIN='main-current',KEYLISTS_EVIDENCE='evidence2')
harness=root/'src/lane-keylists'
if a.pause_child:
    Path('/root/MEASURE.holder').write_text('keylists2 '+datetime.datetime.now(datetime.timezone.utc).isoformat()+'\n')
    env.update(KEYLISTS_CPUS='56-63',KEYLISTS_RECORD_NAME='pause-'+a.name+'.json')
    subprocess.run(['taskset','-c','56-63','setarch','-R','python3','run.py',
                    'measure','obs-main','obs-head','--n','5','--only',*a.only],
                   cwd=harness,env=env,check=True)
else:
    for arm in ['obs-main','obs-head']:
        subprocess.run(['python3','relink.py',arm,'--only',*a.only],cwd=harness,
                       env=env,check=True)
    env['KEYLISTS_RECORD_NAME']='ab-'+a.name+'.json'
    subprocess.run(['python3','run.py','measure','obs-main','obs-head',
                    '--n','5','--only',*a.only],cwd=harness,env=env,check=True)
    subprocess.run(['flock','/root/MEASURE.lock','python3',str(Path(__file__).resolve()),
                    '--name',a.name,'--only',*a.only,'--pause-child'],
                   cwd=harness,env=env,check=True)
