#!/usr/bin/env python3
"""Reuse the existing literal projection for shared constructor-length arms."""
import argparse
import json
import re
from pathlib import Path
import realm_source_migration as m
from realm_metadata_migration import human_length
from realm_constructor_shape_migration import shape_facts
from realm_callable_members_migration import specialize

def bindings(source):
    ts=list(m.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=='fn'and ts[i+1][0]=='builtin_constructor_spec_length')
    match=next(i for i in range(start,len(ts))if ts[i][0]=='match')
    opening=next(i for i in range(match,len(ts))if ts[i][0]=='{')
    end=m.matching(ts,opening);result={}
    for arm in m.split(ts[opening+1:end]):
        pair=m.split(arm,'=>')
        if len(pair)!=2:continue
        value=pair[1]
        if len(value)!=5 or m.joined(value[1:])!='!(length)':continue
        for pattern in m.split(pair[0],'|'):
            if len(pattern)==1 and pattern[0][0].startswith('"'):result[json.loads(pattern[0][0])]=value[0][0]
    return result

def migrate(root,baseline,output,apply=False):
    names=bindings((root/'crates/perry-runtime/src/object/global_this_tables.rs').read_text())
    original=m.simple_facts
    shaped=lambda d:shape_facts(d,original)or specialize(d,lambda x:shape_facts(x,original))
    def classify(d):
        if d.get('kind')!='constructor' or d['name']not in names:return None
        m.simple_facts=shaped
        try:result=human_length(d)
        finally:m.simple_facts=classify
        if result is None:return None
        facts,length=result
        facts['spec']='{length = '+str(length)+'}';facts['emit']='metadata('+names[d['name']]+')'
        return facts
    m.simple_facts=classify
    try:m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
