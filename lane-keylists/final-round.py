#!/usr/bin/env python3
"""Run one coherent, current-base round without sharing targets between arms."""
import argparse
import concurrent.futures
import datetime
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
p = argparse.ArgumentParser()
p.add_argument('--name', required=True)
p.add_argument('--wait-probe', action='store_true')
p.add_argument('--pause-child', action='store_true')
p.add_argument('--existing-main', action='store_true')
p.add_argument('--probe-child', action='store_true')
p.add_argument('--head-only', action='store_true')
p.add_argument('--parallel-units', action='store_true')
p.add_argument('--reuse-current-frontends', metavar='COMPLETED_ROUND')
p.add_argument('--wait-current-frontends', action='store_true')
p.add_argument('--build-jobs', type=int, choices=[1,2,3,4])
a = p.parse_args()
env = dict(os.environ, KEYLISTS_MAIN='main-current', KEYLISTS_EVIDENCE='evidence2')
if a.reuse_current_frontends:
    assert a.head_only and a.parallel_units
    marker=ROOT/'evidence2'/('completed-'+a.reuse_current_frontends+'.json')
    if not a.wait_current_frontends:
        assert json.loads(marker.read_text())['complete'], marker
    # Runtime-only review rounds reuse the freshly generated baseline objects.
    # Refuse reuse if any compiler source differs, or if the population vanished.
    import hashlib
    def frontend_digest(tree):
        digest=hashlib.sha256()
        count=0
        for crate in ['perry','perry-codegen','perry-codegen-common','perry-hir',
                      'perry-parser','perry-transform','perry-types']:
            base=tree/'crates'/crate
            for path in sorted(base.rglob('*')):
                if path.is_file() and (path.suffix=='.rs' or path.name=='Cargo.toml'):
                    digest.update(str(path.relative_to(tree)).encode())
                    digest.update(path.read_bytes())
                    count+=1
        assert count > 100, count
        return digest.hexdigest()
    assert frontend_digest(ROOT/'next-head-src')==frontend_digest(ROOT/'main-current/src')

def command(argv, *, cwd=None, extra=None, log=None, required=True, timeout=7200):
    local = dict(env)
    if extra:
        local.update(extra)
    destination = ROOT / (log or ('keylists2-' + a.name + '-round.log'))
    with destination.open('a') as output:
        result = subprocess.run(argv, cwd=cwd, env=local, stdout=output,
                                stderr=subprocess.STDOUT, timeout=timeout)
    if required and result.returncode:
        raise RuntimeError((argv, result.returncode, destination))
    return result.returncode

def parallel(items):
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(items)) as pool:
        jobs = [pool.submit(fn) for fn in items]
        for job in jobs:
            job.result()

def arm(name):
    base = ROOT if name == 'head' else ROOT / ('main-current' if name == 'main' else name)
    return base, dict(CARGO_TARGET_DIR=str(base/'target'),
                     PERRY_RUNTIME_DIR=str(base/'target/release'),
                     PERRY_WORKSPACE_ROOT=str(base/'src'))

def build(name, shipping):
    base, variables = arm(name)
    jobs=a.build_jobs or (3 if a.parallel_units else 4)
    argv = ['taskset', '-c', '0-23,32-55', 'cargo', 'build', '--manifest-path', str(base/'src/Cargo.toml'), '--release', '-j'+str(jobs)]
    if shipping:
        argv += ['-p', 'perry']
    argv += ['-p', 'perry-runtime', '-p', 'perry-runtime-static', '-p', 'perry-stdlib-static']
    command(argv, cwd=base/'src', extra=variables, log=f'keylists2-{a.name}-{name}-build.log')

def stream(name, jobs=3):
    command(['python3', 'build-stream-graph.py', name], cwd=ROOT/'src/lane-keylists',
            extra={'CARGO_BUILD_JOBS':str(jobs)}, log=f'keylists2-{a.name}-{name}-stream.log')

def units(name):
    base, variables = arm(name)
    if a.parallel_units and name == 'head':
        variables['CARGO_TARGET_DIR']=str(ROOT/'unit-head-target')
    command(['taskset', '-c', '0-23,32-55', 'cargo', 'test', '--manifest-path', str(base/'src/Cargo.toml'), '--release', '-j2',
             '-p', 'perry-runtime', '--lib', '--', '--test-threads=1'],
            cwd=base/'src', extra=variables,
            log=f'keylists2-{a.name}-{name}-units.log', required=False)
    text=(ROOT/f'keylists2-{a.name}-{name}-units.log').read_text()
    if 'test result:' not in text:
        raise RuntimeError(('Unit suite did not execute', name))

harness = ROOT/'src/lane-keylists'
if a.pause_child or a.probe_child:
    Path('/root/MEASURE.holder').write_text('keylists2 ' + datetime.datetime.now(datetime.timezone.utc).isoformat()+'\n')
    only = ['--only', 'root-survival', 'buffer_heavy', 'worker_heavy'] if a.probe_child else []
    label = a.name + ('-probe' if a.probe_child else '')
    command(['taskset', '-c', '56-63', 'python3', 'run.py', 'measure', 'obs-main', 'obs-head', '--n', '5', *only],
            cwd=harness, extra={'KEYLISTS_CPUS':'56-63', 'KEYLISTS_RECORD_NAME':f'pause-{label}.json'},
            log=f'keylists2-{label}-pause.log', timeout=2700)
    # A newer reviewed runtime may have been staged during this focused
    # round. Retain its measurements, but do not start full acceptance builds
    # for an intermediate candidate.
    if a.probe_child:
        source=ROOT/'src/crates/perry-runtime/src'
        staged=ROOT/'next-head-src/crates/perry-runtime/src'
        for path in source.rglob('*.rs'):
            other=staged/path.relative_to(source)
            if not other.exists() or path.read_bytes()!=other.read_bytes():
                raise RuntimeError(('New runtime source awaits its build',path.relative_to(source)))
    raise SystemExit(0)

if a.wait_probe:
    marker = ROOT/'keylists2-strings-probe-5c.log'
    while not marker.exists() or 'worker_heavy measured' not in marker.read_text():
        time.sleep(10)
command(['rsync', '-a', '--delete', '--exclude', 'target', '--exclude', '.git',
         str(ROOT/'next-head-src')+'/', str(ROOT/'src')+'/'])
for name in (['head'] if a.head_only else ['main','head']):
    base, _ = arm(name)
    (base/'src/crates/perry-runtime/src/lib.rs').touch()
    command(['python3', 'prepare-observation.py', name, '--refresh'], cwd=harness)
for name in (['obs-head'] if a.head_only else ['obs-main','obs-head']):
    base, _ = arm(name)
    (base/'src/crates/perry-runtime/src/lib.rs').touch()
if a.reuse_current_frontends:
    if a.wait_current_frontends:
        assert a.build_jobs==1, 'reserve six jobs for the active frontend batch'
        parallel([lambda: build('head',True),lambda: build('obs-head',False)])
        parallel([lambda: stream('head',1),lambda: stream('obs-head',1)])
        units('head')
    else:
        parallel([lambda: build('head',True),lambda: build('obs-head',False),lambda: units('head')])
        parallel([lambda: stream('head',a.build_jobs or 4),lambda: stream('obs-head',a.build_jobs or 4)])
elif a.existing_main:
    if a.head_only:
        if a.parallel_units:
            cache=ROOT/'unit-head-target'
            if not cache.exists():
                command(['cp','-a','--reflink=auto',str(ROOT/'target'),str(cache)])
            parallel([lambda: build('head',True),lambda: build('obs-head',False),lambda: units('head')])
        else:
            parallel([lambda: build('head', True), lambda: build('obs-head', False)])
            units('head')
    else:
        parallel([lambda: build('head', True), lambda: build('obs-main', False)])
        parallel([lambda: build('obs-head', False), lambda: units('head'), lambda: units('main')])
    # Saved frontend objects suffice for diagnosis because compiler and codegen
    # have no diff. Acceptance below always refreshes them on current main.
    only = ['--only', 'root-survival', 'buffer_heavy', 'worker_heavy']
    for name in ['obs-main', 'obs-head']:
        command(['python3', 'relink.py', name, *only], cwd=harness,
                log=f'keylists2-{a.name}-probe.log')
    command(['python3','run.py','measure','obs-main','obs-head','--n','5',*only], cwd=harness,
            extra={'KEYLISTS_RECORD_NAME':f'ab-{a.name}-probe.json'},
            log=f'keylists2-{a.name}-probe.log')
    command(['flock','/root/MEASURE.lock','python3',str(harness/'final-round.py'),
             '--name',a.name,'--probe-child'], cwd=harness)
    import statistics
    rows=json.loads((ROOT/'evidence2'/f'ab-{a.name}-probe.json').read_text())
    pauses=json.loads((ROOT/'evidence2'/f'pause-{a.name}-probe.json').read_text())
    violations=[]
    for name in only[1:]:
        v=[statistics.median(r['instructions'] for r in rows if r['name']==name and r['arm']==arm)
           for arm in ['obs-main','obs-head']]
        delta=100*(v[1]/v[0]-1)
        print(name, 'probe instruction delta', delta, flush=True)
        if delta > .5:
            violations.append((name,delta))
        v=[statistics.median(r['gc_pause_us'] for r in pauses if r['name']==name and r['arm']==arm)
           for arm in ['obs-main','obs-head']]
        delta=100*(v[1]/v[0]-1)
        print(name, 'probe GC-pause delta', delta, flush=True)
        if delta > .5:
            violations.append((name,'GC pause',delta))
    if violations:
        raise RuntimeError(('Focused probe requires another structural fix',violations))
    parallel([lambda: stream('head',2), lambda: stream('main',2),
              lambda: stream('obs-head',2), lambda: stream('obs-main',2)])
else:
    parallel([lambda: build('head', True), lambda: build('main', True)])
    parallel([lambda: build('obs-head', False), lambda: build('obs-main', False)])
    parallel([lambda: stream('head'), lambda: stream('main'), lambda: units('head')])
    parallel([lambda: stream('obs-head'), lambda: stream('obs-main'), lambda: units('main')])
if a.reuse_current_frontends:
    marker=ROOT/'evidence2'/('completed-'+a.reuse_current_frontends+'.json')
    while not marker.exists():
        time.sleep(5)
    assert json.loads(marker.read_text())['complete'], marker
else:
    command(['python3','run.py','compile','main'], cwd=harness, log=f'keylists2-{a.name}-frontend.log')
for name in ['obs-main','obs-head']:
    command(['python3','relink.py',name], cwd=harness, log=f'keylists2-{a.name}-relink.log')
command(['python3','run.py','measure','obs-main','obs-head','--n','5'], cwd=harness,
        extra={'KEYLISTS_RECORD_NAME':f'ab-{a.name}.json'}, log=f'keylists2-{a.name}-ab.log')
command(['flock','/root/MEASURE.lock','python3',str(harness/'final-round.py'),
         '--name',a.name,'--pause-child'], cwd=harness)
previous=ROOT/'evidence2/stress.json'
if previous.exists():
    (ROOT/'evidence2/stress-before-current-round.json').write_bytes(previous.read_bytes())
command(['python3','stress.py','obs-head'], cwd=harness, log=f'keylists2-{a.name}-stress.log')
(ROOT/'evidence2'/('completed-'+a.name+'.json')).write_text(json.dumps({'complete':True})+'\n')
