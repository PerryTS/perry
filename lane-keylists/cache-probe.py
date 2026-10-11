#!/usr/bin/env python3
"""Prepare source-only O5 experiments in this lane's host directory.

No runtime switch is shipped. Each probe has its own source and target.
The transition probe leaves Array-subclass tail history untouched.
"""
from pathlib import Path
import argparse, shutil, subprocess
ROOT=Path('/root/codex-lanes/cx-keylists')
def body(text,name):
 start=text.index('{',text.index('fn '+name+'(')); depth=1; end=start+1
 while depth:
  depth+=(text[end]=='{')-(text[end]=='}');end+=1
 return start,end
p=argparse.ArgumentParser();p.add_argument('arm',choices=['no-transition','no-shape']);a=p.parse_args()
base=ROOT/a.arm
assert not base.exists(),base
base.mkdir()
shutil.copytree(ROOT/'src',base/'src',ignore=shutil.ignore_patterns('.git','target','__pycache__'))
subprocess.run(['cp','-a','--reflink=auto',str(ROOT/'target'),str(base/'target')],check=True)
f=base/'src/crates/perry-runtime/src/object/mod.rs'
s=f.read_text()
if a.arm=='no-transition':
 start,end=body(s,'transition_cache_insert')
 old=s[start:end]
 tail=old[old.index('    if !array_tail_owner.is_null()'):old.index('    // Eagerly stabilize')]
 s=s[:start]+'{\n'+tail+'}'+s[end:]
else:
 start,end=body(s,'shape_cache_get_with_id')
 s=s[:start]+'{ (ObjectKeys::NONE, 0) }'+s[end:]
 start,end=body(s,'shape_cache_insert')
 old=s[start:end]
 # Keep canonical mint/COW semantics. Remove cache publication and its
 # runtime ShapeId accelerator, whose only consumer was the cache entry.
 old=old[:old.index('    let runtime_shape_id =')]
 s=s[:start]+old+'    (live, keys)\n}'+s[end:]
f.write_text(s)
print(base,flush=True)
