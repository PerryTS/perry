#!/usr/bin/env python3
"""Move native callable aliases to their physical declaration, preserving dispatch names."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m
import realm_native_length_migration as lengths
from realm_callable_members_migration import specialize

def alias_facts(d,classify):
    captures=d.get('captures',[])
    if len(captures)!=3 or captures[1]['kind']!='bytes':return None
    label=captures[1]['value']
    prop=next((p for p in d['properties']if p['key']=='name'),None)
    if not prop or prop['value']!=dict(kind='string',value=label):return None
    primary=d['name'].rsplit('.',1)[0]+'.'+label
    if primary==d['name'] or min(primary,d['name'])!=d['name']:return None
    candidate=copy.deepcopy(d);candidate['name']=primary
    facts=classify(candidate)
    if facts is None:return None
    facts['birth_name']=primary;facts['alias']=d['name']
    return facts

def migrate(root,baseline,output,apply=False):
    original=m.native_default_facts
    def classify(d):
        return alias_facts(d,original) or specialize(d,lambda x:alias_facts(x,original))
    m.native_default_facts=classify
    try:
        first=m.migrate_native_defaults(root,baseline,output/'defaults',apply)
        lengths.migrate(root,baseline,output/'lengths',apply)
        return first
    finally:m.native_default_facts=original
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
