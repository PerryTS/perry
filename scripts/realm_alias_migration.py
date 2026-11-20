#!/usr/bin/env python3
"""Migration record: callable aliases declare one physical cell at its body."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m

def alias_facts(d, simple):
    if d.get('kind')!='method' or d.get('captures')!=[]:return None
    name=next((p['value'].get('value') for p in d.get('properties',[]) if p['key']=='name' and p['value']['kind']=='string'),None)
    if not name or name==m.visible_name(d['name']) or not name.isidentifier():return None
    birth=d['name'].rsplit('.',1)[0]+'.'+name
    if min(d['name'],birth)!=d['name']:return None
    candidate=copy.deepcopy(d)
    candidate['name']=birth
    facts=simple(candidate)
    if facts is None:return None
    facts.update(birth_name=birth,alias=d['name'])
    return facts

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    m.simple_facts=lambda d:alias_facts(d,original)
    try:return m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1])
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--apply',action='store_true')
    a=parser.parse_args()
    migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
