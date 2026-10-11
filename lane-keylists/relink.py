#!/usr/bin/env python3
"""Reuse unchanged frontend objects with each arm's private runtime graph."""
import argparse, hashlib, json, os, subprocess, time
from pathlib import Path
import run as h
if (h.ROOT/'tmp/review.pending').exists():
 raise RuntimeError('Candidate runtime review is pending; fresh frontends remain retained')
while (h.ROOT/'tmp/focused-probe.running').exists() and os.environ.get('KEYLISTS_FOCUSED_PROBE')!='1':
 time.sleep(10)
def frontend_digest(tree):
 digest=hashlib.sha256()
 count=0
 for crate in ['perry','perry-codegen','perry-codegen-common','perry-hir',
               'perry-parser','perry-transform','perry-types']:
  for file in sorted((tree/'crates'/crate).rglob('*')):
   if file.is_file() and (file.suffix=='.rs' or file.name=='Cargo.toml'):
    digest.update(str(file.relative_to(tree)).encode())
    digest.update(file.read_bytes())
    count+=1
 assert count>100,count
 return digest.hexdigest()
assert frontend_digest(h.ROOT/'src')==frontend_digest(h.MAIN/'src'), 'Frontend sources differ'
p=argparse.ArgumentParser();p.add_argument('arm',choices=['obs-main','obs-head','phase-main','phase-head']);p.add_argument('--only',nargs='+');a=p.parse_args()
out,env=h.setup_shim(a.arm)
base,_=h.arm_env(a.arm)
env['KEYLISTS_OBSERVER_OBJECT']=str(out/'gc-observe.o')
source=h.EVIDENCE/'main'
statuses={}
for name,(directory,ts,argv) in h.PROGRAMS.items():
 if a.only and name not in a.only:continue
 f=source/(name+'.link.json')
 if not f.exists():raise RuntimeError(('missing saved frontend objects',name))
 args=json.loads(f.read_text())
 for i,v in enumerate(args):
  if v==str(source/'gc-observe.o'):args[i]=str(out/'gc-observe.o')
  elif v.startswith(str(source/'observe-runtime')):args[i]=str(out/'observe-runtime')+v[len(str(source/'observe-runtime')):]
  elif v.startswith(str(h.MAIN)+'/') and v.endswith('.a'):args[i]=str(base)+v[len(str(h.MAIN)):]
 args[args.index('-o')+1]=str(out/(name+'.observed'))
 env['KEYLISTS_PURE_OUTPUT']=str(out/name)
 subprocess.run([str(h.ROOT/'src/lane-keylists/cc'),*args],env=env,check=True)
 cwd=h.ROOT/'drivers'/directory
 oracle=ts.replace('.ts','.js') if directory=='micros' else ts
 node=h.run([*h.NODE,oracle,*argv],cwd,env,out/(name+'.node'))
 actual=h.run(['taskset','-c',h.CPUS,str(out/(name+'.observed')),*argv],cwd,env,out/(name+'.check'))
 pure=h.run(['taskset','-c',h.CPUS,str(out/name),*argv],cwd,env,out/(name+'.pure-check'))
 row=dict(compile_rc=0,node_rc=node.returncode,run_rc=actual.returncode,pure_rc=pure.returncode,equal=h.normalized(node.stdout)==h.normalized(actual.stdout)==h.normalized(pure.stdout))
 assert row['equal'] and row['node_rc']==row['run_rc']==row['pure_rc']==0,(name,row)
 statuses[name]=row
 print(a.arm,name,row,flush=True)
 f=out/'status.json';previous=json.loads(f.read_text()) if f.exists() else {};previous.update(statuses);f.write_text(json.dumps(previous,indent=2)+'\n')
