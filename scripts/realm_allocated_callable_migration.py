#!/usr/bin/env python3
"""Declare named closures created by the intrinsic allocator."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m

def allocated_facts(d,classify):
    if d.get('captures')!=[] or d.get('header',{}).get('reserved')!=m.CONSTANTS['GC_LAYOUT_POINTER_FREE']:return None
    candidate=copy.deepcopy(d);candidate['header']['reserved']=0
    facts=classify(candidate)
    if facts is None or 'provider'in facts:return None
    facts['provider']='intrinsic_closure'
    return facts

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    m.simple_facts=lambda d:allocated_facts(d,original)
    try:m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
