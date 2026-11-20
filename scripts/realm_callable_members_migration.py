#!/usr/bin/env python3
"""Move callable own members to the declaration of their physical function cell."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m
import realm_native_length_migration as lengths
import realm_prototype_birth_migration as p

def specialize(d,classify):
    if 'function_info'not in d:return None
    intrinsic={'length','name'}|({'prototype'}if d['kind']=='constructor'else set())
    extra=[v for v in d['properties']if v['symbol']or v['key']not in intrinsic]
    if not extra:return None
    keys=[k for k in d['own_key_order']if k not in ('length','name','prototype')]
    prefix=[k for k in d['own_key_order']if k in ('length','name','prototype')]
    reordered=d['own_key_order']!=prefix+keys
    if reordered and (prefix!=['length','name','prototype'] or d['own_key_order'][:2]!=['length','name']):return None
    candidate=copy.deepcopy(d)
    candidate['properties']=[v for v in d['properties']if v not in extra]
    candidate['own_key_order']=prefix;candidate['own_symbol_order']=[]
    facts=classify(candidate)
    if facts is None:return None
    if reordered:
        extra=extra+[dict(key='prototype',symbol=False,attributes=5,value=dict(kind='reference',value=d['name']+'.prototype'))]
        keys=d['own_key_order'][2:]
    rows,edges=p.members(dict(name=d['name'],properties=extra,own_key_order=keys,own_symbol_order=d['own_symbol_order']))
    facts['members']='{ '+'; '.join(rows)+'; }'
    implied=edges|{d.get('prototype')}
    if d['kind']=='constructor':
        implied.add(next(v['value']['value']for v in d['properties']if v['key']=='prototype'))
    births=[b for b in facts.get('births',[])if b not in implied]
    if births:facts['births']=births
    else:facts.pop('births',None)
    return facts

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts;native=m.native_default_facts
    m.simple_facts=lambda d:specialize(d,original)
    m.native_default_facts=lambda d:specialize(d,native)
    try:
        m.migrate(root,baseline,output/'direct',apply)
        m.migrate_native_defaults(root,baseline,output/'native',apply)
        lengths.migrate(root,baseline,output/'native-lengths',apply)
    finally:m.simple_facts=original;m.native_default_facts=native
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
