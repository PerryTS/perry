#!/usr/bin/env python3
"""Move exceptional public names into body metadata and project their birth literals."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m
from realm_constructor_shape_migration import shape_facts
from realm_callable_members_migration import specialize

GROUPS=[
 ('%AsyncFunctionPrototype%.constructor','realm_async_function_name','object/global_this/generator.rs','set_bound_native_closure_name(ctor, "AsyncFunction");','set_bound_native_closure_name(ctor, {macro}!(name));'),
 ('%AsyncGeneratorFunctionPrototype%.constructor','realm_async_generator_name','object/global_this/generator.rs','(\n            "AsyncGeneratorFunction",\n            "AsyncGeneratorFunction",','(\n            {macro}!(name),\n            "AsyncGeneratorFunction",'),
 ('%GeneratorFunctionPrototype%.constructor','realm_generator_name','object/global_this/generator.rs','("GeneratorFunction", "GeneratorFunction", "Generator")','({macro}!(name), "GeneratorFunction", "Generator")'),
 ('node:events.EventEmitter.EventEmitterAsyncResource.<get>','realm_event_resource_get_name','object/native_module/constructor_shapes.rs','set_bound_native_closure_name(p, "lazyEventEmitterAsyncResource")','set_bound_native_closure_name(p, {macro}!(name))'),
 ('node:zlib.Deflate.prototype.<prototype>.<prototype>._closed.<get>','realm_zlib_closed_get_name','object/native_module/constructor_shapes.rs','set_bound_native_closure_name(p, "get")','set_bound_native_closure_name(p, {macro}!(name))'),
 ('node:fs.default.promises.<get>','realm_fs_promises_get_name','object/native_module/callable_exports.rs','if property_name == "promises" {\n        "get".to_string()','if property_name == "promises" {\n        {macro}!(name).to_string()'),
]
def replace_literal(source,old,new):
    assert source.count(old)==1
    assert old.count('\n')==new.count('\n')
    return source.replace(old,new,1)

def migrate(root,baseline,output,apply=False):
    names={name:macro for name,macro,*_ in GROUPS}
    original=m.simple_facts
    def facts(d):
        if d['name']not in names:return None
        candidate=copy.deepcopy(d)
        label=next(p['value']['value']for p in candidate['properties']if p['key']=='name')
        for prop in candidate['properties']:
            if prop['key']=='name':prop['value']['value']=m.visible_name(d['name'])
        shaped=lambda x:shape_facts(x,original)
        human=shaped(candidate)
        if human is None:human=specialize(candidate,shaped)
        assert human is not None,d['name']
        human.update(spec='{name = '+json.dumps(label)+'}',emit='metadata('+names[d['name']]+')')
        return human
    for _,macro,file,old,new in GROUPS:
        source=(root/'crates/perry-runtime/src'/file).read_text()
        assert source.count(old)==1,(file,old)
        assert old.count('\n')==new.count('\n')
    m.simple_facts=facts
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==len(GROUPS)
    if apply:
        for _,macro,file,old,new in GROUPS:
            path=root/'crates/perry-runtime/src'/file
            path.write_text(replace_literal(path.read_text(),old,new.replace('{macro}',macro)))
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
