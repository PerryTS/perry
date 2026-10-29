#!/usr/bin/env python3
"""Project remaining public lengths from their body declarations into existing births."""
import argparse
import json
from pathlib import Path
import realm_source_migration as m
from realm_metadata_migration import human_length

GROUPS=[
    (['WebAssembly.Global','WebAssembly.Instance'],'realm_wasm_constructor_public','object/global_this_webassembly.rs','set_builtin_closure_length(closure as usize, 1);','set_builtin_closure_length(closure as usize, {macro}!(length));'),
    (['Intl.Locale'],'realm_locale_constructor_public','intl/locale.rs','set_builtin_closure_length(ctor as usize, 1);','set_builtin_closure_length(ctor as usize, {macro}!(length));'),
    (['BigInt.prototype.toString'],'realm_bigint_to_string_public','object/primitive_proto_thunks.rs','crate::value::js_nanbox_get_pointer(to_string) as usize,\n                0,','crate::value::js_nanbox_get_pointer(to_string) as usize,\n                {macro}!(length),'),
    (['ArrayBuffer.prototype.transfer','ArrayBuffer.prototype.transferToFixedLength'],'realm_arraybuffer_transfer_public','object/global_this/proto_methods.rs','set_builtin_closure_length(closure, 0);','set_builtin_closure_length(closure, {macro}!(length));'),
    (['node:module.SourceMap.prototype.findEntry','node:module.SourceMap.prototype.findOrigin'],'realm_source_map_method_public','process/node_module/source_map.rs','set_builtin_closure_length(closure as usize, 2);','set_builtin_closure_length(closure as usize, {macro}!(length));'),
    (['node:module.Module.prototype.load','node:module.Module.prototype.require'],'realm_module_method_public','object/native_module/callable_exports/module_cjs.rs','[(0, "_compile", 3), (3, "load", 1), (5, "require", 1)]','[(0, "_compile", 3), (3, "load", {macro}!(length)), (5, "require", {macro}!(length))]'),
    (['node:async_hooks.AsyncLocalStorage.prototype.enterWith','node:async_hooks.AsyncLocalStorage.prototype.exit'],'realm_async_scope_public','object/native_module/async_hooks_exports.rs','("enterWith", 1),\n    ("exit", 1),','("enterWith", {macro}!(length)),\n    ("exit", {macro}!(length)),'),
    (['node:async_hooks.AsyncLocalStorage.prototype.run'],'realm_async_run_public','object/native_module/async_hooks_exports.rs','("run", 2),','("run", {macro}!(length)),'),
    (['node:tls.TLSSocket.prototype.setKeyCert'],'realm_tls_setkey_public','object/native_module/callable_exports.rs','("setKeyCert", 1),','("setKeyCert", {macro}!(length)),'),
]

def replacement(source,old,new):
    assert source.count(old)==1,(old,source.count(old))
    result=source.replace(old,new,1)
    assert source.count('\n')==result.count('\n')
    return result

def migrate(root,baseline,output,apply=False):
    targets={name:macro for names,macro,*_ in GROUPS for name in names}
    original=m.simple_facts
    def facts(d):
        if d['name']not in targets:return None
        # human_length delegates to simple_facts for the ABI-normalized candidate.
        m.simple_facts=original
        try:value=human_length(d)
        finally:m.simple_facts=facts
        assert value is not None,d['name']
        human,length=value
        human.update(spec='{length = '+str(length)+'}',emit='metadata('+targets[d['name']]+')')
        return human
    # Verify all operational replacements before mutating any source.
    for _,macro,file,old,new in GROUPS:
        replacement((root/'crates/perry-runtime/src'/file).read_text(),old,new.format(macro=macro))
    m.simple_facts=facts
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert {p['name']for p in report['placements']}==set(targets)
    if apply:
        for _,macro,file,old,new in GROUPS:
            path=root/'crates/perry-runtime/src'/file
            path.write_text(replacement(path.read_text(),old,new.format(macro=macro)))
        for file,signature in [('intl/locale.rs','pub(super) fn install_locale('),('object/global_this_webassembly.rs','fn install_webassembly_constructor('),('process/node_module/source_map.rs','fn source_map_method(')]:
            path=root/'crates/perry-runtime/src'/file
            source=path.read_text();assert source.count(signature)==1
            path.write_text(source.replace(signature,'#[rustfmt::skip] '+signature))
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
