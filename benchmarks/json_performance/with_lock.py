#!/usr/bin/env python3
"""Run on the shared benchmark Mac; never replace another run's lock."""
from pathlib import Path
import json, os, subprocess, sys, time
root=Path(__file__).resolve().parent
custom_command=sys.argv[2:] if sys.argv[1:2]==['--'] else None
if custom_command==[]: raise SystemExit('Expected a command after --')
phases=['custom'] if custom_command else sys.argv[1:] or ['timing','memory','tape']
lock=Path.home()/'bench.lock'
token='json-perf-'+str(os.getpid())+'-'+str(int(time.time()))
try: lock.mkdir()
except FileExistsError:
    print('BUSY', (lock/'owner').read_text());sys.exit(2)
(lock/'owner').write_text(token)
meta={'owner':token,'load_before':os.getloadavg(),'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())}
if custom_command: meta['command']=custom_command
try:
    print('ACQUIRED',token,'load',os.getloadavg(),flush=True)
    if os.getloadavg()[0]>2.5: raise RuntimeError('Host too busy for timing')
    ps=subprocess.check_output(['ps','-Ao','pid,pcpu,comm'],text=True)
    if any('/rustc' in line or '/cargo' in line for line in ps.splitlines()): raise RuntimeError('Compiler running on measurement host')
    (root/'results/processes-before.txt').write_text(ps)
    for phase in phases:
        with (root/(phase+'.log')).open('w') as log:
            cmd=custom_command or (['python3',str(root/(phase+'.py'))] if phase in ['diagnostics','profiles','fastpaths'] else ['python3',str(root/'run.py'),'--phase',phase])
            subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,check=True)
        print('FINISHED',phase,'load',os.getloadavg(),flush=True)
    meta['load_after']=os.getloadavg()
    ps=subprocess.check_output(['ps','-Ao','pid,pcpu,comm'],text=True)
    (root/'results/processes-after.txt').write_text(ps)
    meta['quiet_gate_passed']=meta['load_after'][0]<=2.5 and not any('/rustc' in line or '/cargo' in line for line in ps.splitlines())
    print('QUIET_GATE',meta['quiet_gate_passed'],flush=True)
finally:
    meta['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    (root/('results/window-'+'-'.join(phases)+'.json')).write_text(json.dumps(meta,indent=2)+'\n')
    if (lock/'owner').read_text()==token:
        (lock/'owner').unlink();lock.rmdir();print('RELEASED',flush=True)
