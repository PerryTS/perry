#!/usr/bin/env python3
"""Move Storage public metadata into body declarations and project original literals."""
import argparse
import json
from pathlib import Path
import realm_source_migration as m
from realm_metadata_migration import human_length,human_name

def storage_facts(d):
    name=d['name']
    if name.startswith('Storage.prototype.'):
        result=human_length(d)
        if result is None:return None
        facts,length=result
        if length!=0:raise ValueError('Storage installer is no longer zero-arity')
        facts.update(spec='{length = 0}',emit='metadata(realm_storage_method)')
        return facts
    if name in ('localStorage.<get>','sessionStorage.<get>'):
        result=human_name(d)
        if result is None:return None
        facts,label=result
        if label!=name.removesuffix('.<get>'):raise ValueError('Storage getter no longer uses its property name')
        facts.update(spec='{name = property_key}',emit='metadata(realm_'+label+')')
        return facts
    return None

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    def classify(d):
        m.simple_facts=original
        try:return storage_facts(d)
        finally:m.simple_facts=classify
    m.simple_facts=classify
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    if apply:
        p=root/'crates/perry-runtime/src/web_storage.rs';s=p.read_text()
        old='set_builtin_closure_length(closure as usize, 0)'
        assert s.count(old)==2
        s=s.replace(old,'set_builtin_closure_length(closure as usize, realm_storage_method!(length))')
        for name in ('localStorage','sessionStorage'):
            old='set_global_storage_property(global, "'+name+'",'
            assert s.count(old)==1
            s=s.replace(old,'set_global_storage_property(global, realm_'+name+'!(name),')
        p.write_text(s)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
