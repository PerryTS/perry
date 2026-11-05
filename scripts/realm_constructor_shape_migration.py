#!/usr/bin/env python3
"""Migrate nondefault constructor inheritance and instance-property attributes."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m
import realm_metadata_migration as metadata
from realm_callable_members_migration import specialize

def shape_facts(d,classify):
    if d.get('kind')!='constructor':return classify(d)
    candidate=copy.deepcopy(d)
    candidate['prototype']='Function.prototype'
    prop=next((p for p in candidate['properties']if p['key']=='prototype'),None)
    if prop is None or prop['value']['kind']!='reference':return None
    attrs=prop['attributes'];instance=prop['value']['value']
    prop['attributes']=0;prop['value']['value']=d['name']+'.prototype'
    facts=classify(candidate)
    if facts is None:return None
    if d['prototype']!='Function.prototype':facts['extends']=d['prototype']
    if attrs:facts['attrs']='{prototype = '+' | '.join(n for bit,n in [(1,'writable'),(2,'enumerable'),(4,'configurable')]if attrs&bit)+'}'
    if instance!=d['name']+'.prototype':facts['instance']=instance
    implied={d['prototype'],instance}
    births=[b for b in facts.get('births',[])if b not in implied]
    if births:facts['births']=births
    else:facts.pop('births',None)
    return facts

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    m.simple_facts=lambda d:shape_facts(d,original) or specialize(d,lambda x:shape_facts(x,original))
    try:
        m.migrate(root,baseline,output/'direct',apply)
        metadata.migrate(root,baseline,output/'lengths',apply,constructors=True)
        metadata.migrate(root,baseline,output/'literal-lengths',apply)
    finally:m.simple_facts=original
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
