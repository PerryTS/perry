#!/usr/bin/env python3
"""Put reference captures and bare constructor metadata on their existing body infos."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m
import realm_info_expression_migration as expr
from realm_constructor_shape_migration import shape_facts
from realm_callable_members_migration import specialize

TARGETS={'Buffer','%TypedArray%','Buffer["@@species"].<get>','node:readline/promises.Interface'}
def classify(d,original):
    if d['name']not in TARGETS:return None
    candidate=copy.deepcopy(d);refs=[c['value']for c in d['captures']if c['kind']=='reference']
    if len(refs)!=len(d['captures']):return None
    candidate['captures']=[]
    family=d.get('constructor')
    if family is not None:
        identity=family['identity'];candidate['captures']=[dict(kind='int32',value=identity)]
        candidate['header']['capture_flags']=(identity<<m.CONSTANTS['CLOSURE_CONSTRUCT_SHIFT'])|m.CONSTANTS['CLOSURE_NO_THIS_REBIND_FLAG']
    if d['name']=='%TypedArray%':
        for prop in candidate['properties']:
            if prop['key']=='name':prop['value']['value']=m.visible_name(d['name'])
    shaped=lambda x:shape_facts(x,original)
    human=shaped(candidate)
    if human is None:human=specialize(candidate,shaped)
    assert human is not None,d['name']
    if refs:
        human.pop('provider',None)
        human['capture']='['+', '.join('ref('+m.render_path(ref)+')'for ref in refs)+']'
        births=[b for b in human.get('births',[])if b not in refs]
        if births:human['births']=births
        else:human.pop('births',None)
    if d['name']=='%TypedArray%':human.update(spec='{name = "TypedArray"}',emit='metadata(realm_typed_array_public)')
    return human

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    m.simple_facts=lambda d:classify(d,original)
    pending={d['name']for path in root.glob('crates/*/src/**/*.realm.json')for rows in json.loads(path.read_text())['groups'].values()for d in rows}&TARGETS
    try:
        report=m.migrate(root,baseline,output/'direct',apply)
        typed=expr.migrate(root,baseline,output/'typed',apply)
        report['placements']+=typed['placements']
    finally:m.simple_facts=original
    assert {p['name']for p in report['placements']}==pending
    if apply:
        path=root/'crates/perry-runtime/src/object/global_this/typed_array.rs'
        source=path.read_text();old='set_bound_native_closure_name(ctor, "TypedArray");'
        assert source.count(old)==1 or 'set_bound_native_closure_name(ctor, realm_typed_array_public!(name));' in source
        source=source.replace(old,'set_bound_native_closure_name(ctor, realm_typed_array_public!(name));')
        if '#[rustfmt::skip] pub(crate) fn ensure_typed_array_intrinsic(' not in source:source=source.replace('pub(crate) fn ensure_typed_array_intrinsic(','#[rustfmt::skip] pub(crate) fn ensure_typed_array_intrinsic(',1)
        path.write_text(source)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
