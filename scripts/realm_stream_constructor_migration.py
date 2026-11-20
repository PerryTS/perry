#!/usr/bin/env python3
"""Project stream constructor public lengths into their existing birth fallback."""
import argparse
import copy
import json
import struct
from pathlib import Path
import realm_source_migration as m
from realm_callable_members_migration import specialize
from realm_constructor_shape_migration import shape_facts
NAMES={'ReadableStream','WritableStream','TransformStream'}
def classify(d,original):
    if d['name']not in NAMES:return None
    c=copy.deepcopy(d)
    length=m.default_length(c['function_info'])
    c['length']=length
    for p in c['properties']:
        if p['key']=='length':p['value']['value']=struct.pack('>d',float(length)).hex()
    simple=lambda d:shape_facts(d,original)
    human=simple(c)
    if human is None:human=specialize(c,simple)
    assert human is not None,d['name']
    human.update(spec='{length = 0}',emit='metadata(realm_stream_constructor_public)')
    return human
def migrate(root,baseline,output,apply=False):
    path=root/'crates/perry-runtime/src/object/global_this/install_static.rs'
    old='.or_else(|| crate::closure::closure_length(ctor))\n        .unwrap_or(0)'
    new='.or_else(|| crate::closure::closure_length(ctor))\n        .unwrap_or(realm_stream_constructor_public!(length))'
    assert path.read_text().count(old)==1
    original=m.simple_facts
    m.simple_facts=lambda d:classify(d,original)
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==3
    if apply:path.write_text(path.read_text().replace(old,new,1))
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
