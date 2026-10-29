#!/usr/bin/env python3
"""Name the existing mock tracker ID at its birth and bind its closure capture."""
import argparse
import copy
import json
import struct
from pathlib import Path
import realm_source_migration as m
NAME='node:test.mock.property'
def classify(d,original):
    if d['name']!=NAME:return None
    assert d['captures']==[dict(kind='raw_word',value='0x0')]
    c=copy.deepcopy(d);c['captures']=[]
    length=m.bare_length(c['function_info']);c['length']=length
    for p in c['properties']:
        if p['key']=='length':p['value']['value']=struct.pack('>d',float(length)).hex()
    human=original(c);assert human is not None
    human.update(capture='[pointer_constant(PROPERTY_TRACKER_ID)]',spec='{length = 3}',emit='metadata(realm_mock_property_public)')
    return human
def migrate(root,baseline,output,apply=False):
    path=root/'crates/perry-runtime/src/node_submodules/test_property.rs'
    original=m.simple_facts;m.simple_facts=lambda d:classify(d,original)
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==1
    if apply:
        s=path.read_text()
        old='pub(super) fn tracker_property_value() -> f64 {'
        assert s.count(old)==1;s=s.replace(old,old+' const PROPERTY_TRACKER_ID: i64 = 0;',1)
        old='        2,\n        0,\n    );\n    let raw = raw_ptr_from_value(value);'
        new='        2,\n        PROPERTY_TRACKER_ID,\n    );\n    let raw = raw_ptr_from_value(value);'
        assert s.count(old)==1;s=s.replace(old,new,1)
        old='crate::object::set_builtin_closure_length(raw, 3);'
        assert s.count(old)==1;s=s.replace(old,'crate::object::set_builtin_closure_length(raw, realm_mock_property_public!(length));',1)
        s=s.replace('pub(super) fn tracker_property_value()', '#[rustfmt::skip] pub(super) fn tracker_property_value()',1)
        path.write_text(s)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
