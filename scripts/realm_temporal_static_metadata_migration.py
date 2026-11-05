#!/usr/bin/env python3
"""Project shared Temporal static-method lengths from their fn_info declarations."""
import argparse
import json
from pathlib import Path
import realm_source_migration as m
from realm_metadata_migration import human_length

def method_length(role):
    return {'from':1,'compare':2}[role]

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    def classify(d):
        if not d['name'].startswith('Temporal.') or d['name'].count('.')!=2:return None
        role=d['name'].rsplit('.',1)[1]
        if role not in ('from','compare'):return None
        m.simple_facts=original
        try:result=human_length(d)
        finally:m.simple_facts=classify
        if result is None:return None
        facts,length=result
        assert length==method_length(role)
        facts.update(spec='{length = '+str(length)+'}',emit='metadata(realm_temporal_static_'+role+')')
        return facts
    m.simple_facts=classify
    try:m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    if apply:
        p=root/'crates/perry-runtime/src/object/global_this/math_temporal.rs';s=p.read_text()
        for role,length in [('from',1),('compare',2)]:
            old='install_constructor_static(ctor, "'+role+'", '+role+'_info, '+str(length)+');'
            assert s.count(old)==1
            s=s.replace(old,'install_constructor_static(ctor, "'+role+'", '+role+'_info, realm_temporal_static_'+role+'!(length));')
        s=s.replace('fn install_temporal_from_compare(', '#[rustfmt::skip] fn install_temporal_from_compare(')
        p.write_text(s)
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
