#!/usr/bin/env python3
"""Rebuild one runtime review while retaining its fresh baseline frontends."""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import time

ROOT=Path(os.environ['KEYLISTS_ROOT'])
env=dict(os.environ,KEYLISTS_MAIN='main-current',KEYLISTS_EVIDENCE='evidence2')
harness=ROOT/'src/lane-keylists'
NAME='current16-minor-review'
def run(argv,cwd,local,log,required=True):
    with (ROOT/('keylists2-'+NAME+'-'+log+'.log')).open('w') as output:
        result=subprocess.run(['taskset','-c','0-23,32-55',*argv],cwd=cwd,
                              env=local,stdout=output,stderr=subprocess.STDOUT)
    if required:assert result.returncode==0,(argv,result.returncode,log)
    return result.returncode
def copy_cache(source,target):
    assert not target.exists(),target
    subprocess.run(['cp','-a','--reflink=auto',str(source),str(target)],check=True)

run(['python3','prepare-observation.py','head','--refresh'],harness,env,'prepare')
def build(arm):
    base=ROOT if arm=='head' else ROOT/'obs-head'
    target=base/'minor-review-target'
    copy_cache(base/'target',target)
    local=dict(env,CARGO_TARGET_DIR=str(target),PERRY_RUNTIME_DIR=str(target/'release'),
               PERRY_WORKSPACE_ROOT=str(base/'src'))
    packages=['-p','perry'] if arm=='head' else []
    run(['cargo','build','--manifest-path',str(base/'src/Cargo.toml'),'--release',
         '-j2',*packages,'-p','perry-runtime','-p','perry-runtime-static',
         '-p','perry-stdlib-static'],base/'src',local,arm+'-build')
def units():
    target=ROOT/'minor-review-unit-target'
    copy_cache(ROOT/'target',target)
    local=dict(env,CARGO_TARGET_DIR=str(target),PERRY_WORKSPACE_ROOT=str(ROOT/'src'))
    run(['cargo','test','--manifest-path',str(ROOT/'src/Cargo.toml'),'--release',
         '-j2','-p','perry-runtime','--lib','--','--test-threads=1'],
        ROOT/'src',local,'head-units',required=False)
    assert 'test result:' in (ROOT/('keylists2-'+NAME+'-head-units.log')).read_text()
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
    jobs=[pool.submit(build,'head'),pool.submit(build,'obs-head'),pool.submit(units)]
    for job in jobs:job.result()
def stream(arm):
    base=ROOT if arm=='head' else ROOT/'obs-head'
    target=base/'minor-review-stream-target'
    copy_cache(base/'src/target/perry-no-auto-stream-dispatch',target)
    local=dict(env,CARGO_TARGET_DIR=str(target),PERRY_RUNTIME_DIR=str(target/'release'),
               PERRY_WORKSPACE_ROOT=str(base/'src'))
    run(['cargo','build','--manifest-path',str(base/'src/Cargo.toml'),'--release',
         '-j3','-p','perry-runtime','-p','perry-runtime-static','-p','perry-stdlib-static',
         '--features','perry-stdlib/external-http-client-pump','-p','perry-ext-http'],
        base/'src',local,arm+'-stream')
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    list(pool.map(stream,['head','obs-head']))
# The older orchestration exits at relink.py's explicit review guard after
# publishing all fresh frontend statuses. Its binaries were never measured.
status=ROOT/'evidence2/main/status.json'
while not status.exists() or len(json.loads(status.read_text()))!=18:
    time.sleep(10)
rows=json.loads(status.read_text())
assert all(r['equal'] and r['compile_rc']==r['node_rc']==r['run_rc']==0 for r in rows.values())
for arm in ['head','obs-head']:
    base=ROOT if arm=='head' else ROOT/'obs-head'
    (base/'target').rename(base/'before-minor-review-target')
    (base/'minor-review-target').rename(base/'target')
    selected=base/'src/target/perry-no-auto-stream-dispatch'
    selected.rename(base/'before-minor-review-stream-target')
    (base/'minor-review-stream-target').rename(selected)
(ROOT/'tmp/review.pending').unlink()
for arm in ['obs-main','obs-head']:
    run(['python3','relink.py',arm],harness,env,arm+'-relink')
local=dict(env,KEYLISTS_RECORD_NAME='ab-'+NAME+'.json')
run(['python3','run.py','measure','obs-main','obs-head','--n','5'],harness,local,'ab')
run(['flock','/root/MEASURE.lock','python3',str(harness/'final-round.py'),
     '--name',NAME,'--pause-child'],harness,env,'pause')
run(['python3','stress.py','obs-head'],harness,env,'stress')
(ROOT/'evidence2'/('completed-'+NAME+'.json')).write_text('{"complete":true}\n')
