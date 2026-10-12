#!/usr/bin/env python3
"""THP-off peak RSS attribution; sample smaps without changing the program."""
import argparse
import ctypes
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import time
spec=importlib.util.spec_from_file_location('common',Path(__file__).resolve().parents[2]/'scripts/perf_objops_lane.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
p=argparse.ArgumentParser();p.add_argument('--hostdir',type=Path,required=True);p.add_argument('--programs',default='fastify,schema-record,worker_heavy,buffer_heavy');a=p.parse_args();root=a.hostdir
if ctypes.CDLL(None,use_errno=True).prctl(41,1,0,0,0):raise OSError('THP disable failed')
results={}
for name in a.programs.split(','):
    relative,args=m.PROGRAMS[name]
    results[name]={}
    for arm in ('main','head'):
        out=root/'measure'/arm/f'{name}.smaps.out';err=root/'measure'/arm/f'{name}.smaps.err'
        peak={}
        with out.open('wb') as stdout,err.open('wb') as stderr:
            process=subprocess.Popen([str(root/'measure'/arm/name),*args],cwd=m.program_cwd(root,relative),env=m.environment(root,arm),stdout=stdout,stderr=stderr)
            while process.poll() is None:
                try:
                    text=Path(f'/proc/{process.pid}/smaps_rollup').read_text()
                    sample={k:int(v) for k,v in re.findall(r'^(\w+):\s+(\d+) kB',text,re.M)}
                    if sample.get('Rss',0)>peak.get('Rss',0):peak=sample
                except (FileNotFoundError,ProcessLookupError):pass
                time.sleep(.002)
        if process.returncode:raise RuntimeError(f'{name}/{arm}: {process.returncode}')
        results[name][arm]=peak
        print(name,arm,{k:peak.get(k,0) for k in ('Rss','Pss_Anon','Pss_File','Anonymous','Private_Dirty','AnonHugePages')},flush=True)
    (root/'measure/rss-attribution.json').write_text(json.dumps(results,indent=2))
