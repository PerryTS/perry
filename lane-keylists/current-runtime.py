#!/usr/bin/env python3
"""Finish a reviewed runtime while the fresh baseline frontend overlaps it."""
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT=Path(os.environ['KEYLISTS_ROOT'])
NAME=os.environ['KEYLISTS_ROUND_NAME']
ENV=dict(os.environ,KEYLISTS_MAIN='main-current',KEYLISTS_EVIDENCE='evidence2')
HARNESS=ROOT/'src/lane-keylists'
running=ROOT/'tmp/runtime-current.running'
assert not running.exists()
running.write_text('two runtime jobs reserved\n')


def run(argv,log,*,base=None,target=None,required=True):
    base=base or ROOT
    target=target or base/'target'
    env=dict(ENV,CARGO_TARGET_DIR=str(target),PERRY_RUNTIME_DIR=str(target/'release'),
             PERRY_WORKSPACE_ROOT=str(base/'src'),CARGO_BUILD_JOBS='2')
    with (ROOT/f'keylists2-{NAME}-{log}.log').open('w') as out:
        result=subprocess.run(['taskset','-c','0-23,32-55',*argv],
                              cwd=base/'src' if argv[0]=='cargo' else HARNESS,
                              env=env,stdout=out,stderr=subprocess.STDOUT)
    if required:assert result.returncode==0,(argv,result.returncode,log)


try:
    resume=os.environ.get('KEYLISTS_RESUME_UNITS')=='1'
    if not resume:
        run(['python3','prepare-observation.py','head','--refresh'],'prepare-head')
        for arm in ['head','obs-head']:
            base=ROOT if arm=='head' else ROOT/arm
            packages=['-p','perry'] if arm=='head' else []
            run(['cargo','build','--release','-j2',*packages,'-p','perry-runtime',
                 '-p','perry-runtime-static','-p','perry-stdlib-static'],
                arm+'-build',base=base)
        for arm in ['head','obs-head']:
            run(['python3','build-stream-graph.py',arm],arm+'-stream')
    else:
        for label in ['head-build','obs-head-build','head-stream','obs-head-stream']:
            assert 'Finished ' in (ROOT/f'keylists2-{NAME}-{label}.log').read_text()
    target=ROOT/'unit-head-target'
    if resume:
        assert target.is_dir()
    else:
        assert not target.exists()
        subprocess.run(['cp','-a','--reflink=auto',str(ROOT/'target'),str(target)],check=True)
    run(['cargo','test','--release','-j2','-p','perry-runtime','--lib','--',
         '--test-threads=1'],'head-units',target=target,required=False)
    assert 'test result:' in (ROOT/f'keylists2-{NAME}-head-units.log').read_text()
finally:
    running.unlink()

while not (ROOT/'evidence2/baseline-ready.json').exists():time.sleep(10)
assert json.loads((ROOT/'evidence2/baseline-ready.json').read_text())['complete']
def failures(arm):
    text=(ROOT/f'keylists2-{NAME}-{arm}-units.log').read_text()
    return set(re.findall(r'^    (\S+)\s*$',text,re.M))
assert failures('main')==failures('head'),(failures('main'),failures('head'))
for arm in ['obs-main','obs-head']:
    run(['python3','relink.py',arm],arm+'-relink')
ENV['KEYLISTS_RECORD_NAME']='ab-'+NAME+'.json'
run(['python3','run.py','measure','obs-main','obs-head','--n','5'],'ab')
run(['flock','/root/MEASURE.lock','python3',str(HARNESS/'final-round.py'),
     '--name',NAME,'--pause-child'],'pause')
run(['python3','stress.py','obs-head'],'stress')
(ROOT/'evidence2'/('completed-'+NAME+'.json')).write_text('{"complete":true}\n')
