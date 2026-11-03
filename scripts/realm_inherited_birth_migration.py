#!/usr/bin/env python3
"""Place shared ordinary and typed-array births; derive inherited-object flags."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m
import realm_prototype_birth_migration as p
from realm_plain_namespace_migration import expression_in_function

def addition(source,a,b,rows,prefix='crate'):
    text='; '.join(rows)+'; '
    ts=list(m.tokens(source))
    owners=[]
    for i,t in enumerate(ts[:-3]):
        if t[0]=='realm_decl'and m.joined(ts[i+1:i+4])=='!{birth':
            end=m.matching(ts,i+2)
            if t[1]<a<ts[end][2]:owners.append((i,end))
    if owners:
        assert len(owners)==1
        start,end=owners[0]
        assert ts[end-1][0]=='}'
        return ts[end-1][1],ts[end-1][1],text,source[:ts[start][1]].count('\n')+1
    return a,b,prefix+'::realm_decl! { birth '+source[a:b]+'; realm { '+text+'} }',source[:a].count('\n')+1

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={f:json.loads(f.read_text())for f in root.glob('crates/*/src/**/*.realm.json')}
    pending={d['name']:d for doc in companions.values()for rows in doc['groups'].values()for d in rows}
    r='crates/perry-runtime/src/'
    groups=[
        (r+'cluster.rs','alloc_object_value','js_object_alloc(CLUSTER_SHAPE_ID, field_count)',['node:cluster.settings','node:cluster.workers']),
        (r+'object/native_module/web_locks.rs','worker_threads_locks_value','js_object_alloc(WORKER_THREADS_LOCK_MANAGER_CLASS_ID, 0)',['node:worker_threads.locks']),
        (r+'object/global_this/typed_array.rs','ensure_typed_array_intrinsic','js_object_alloc(0, 0)',['%TypedArray%.prototype']),
        (r+'process/metadata_values.rs','process_allowed_flags_value','module_set_value(FLAGS)',['process.allowedNodeEnvironmentFlags','node:process.allowedNodeEnvironmentFlags']),
        (r+'object/class_registry/prototype_objects.rs','ensure_function_prototype_object','crate::object::object_alloc_prototype_old(0, 0)',['node:readline/promises.Interface.prototype']),
        (r+'tty.rs','ensure_tty_prototypes','crate::object::js_object_alloc_with_shape(\n        0x7FFF_FF31,\n        2,\n        read_keys.as_ptr(),\n        read_keys.len() as u32,\n    )',['node:tty.ReadStream.prototype']),
        (r+'tty.rs','ensure_tty_prototypes','crate::object::js_object_alloc_with_shape(\n        0x7FFF_FF32,\n        10,\n        write_keys.as_ptr(),\n        write_keys.len() as u32,\n    )',['node:tty.WriteStream.prototype']),
        (r+'node_test.rs','test_assert_object','crate::object::js_object_alloc(0, 0)',['node:test.assert']),
        (r+'object/native_module/web_locks.rs','worker_threads_locks_proto_value','crate::object::js_object_alloc(0, 0)',['node:worker_threads.locks.<prototype>']),
        (r+'object/iterator_prototypes.rs','build_iterator_prototypes','js_object_alloc(0, 0)',['%IteratorPrototype%']),
        (r+'process/env_misc.rs','js_process_env_impl','crate::object::js_object_alloc(0, alloc_limit)',['process.env']),
        (r+'object/native_module/callable_exports/module_cjs.rs','module_cjs_cache_value','crate::object::js_object_alloc_null_proto(0, 0)',['node:module._cache']),
        (r+'object/native_module/callable_exports/module_cjs.rs','module_cjs_path_cache_value','crate::object::js_object_alloc_null_proto(0, 0)',['node:module._pathCache']),
        (r+'cluster.rs','empty_object_value','js_object_alloc(0, 0)',['node:cluster.default._events']),
        (r+'node_inspector.rs','object','js_object_alloc(0, fields.len() as u32)',['node:inspector.console']),
        (r+'node_submodules/mod.rs','submodule_default_object_value','js_object_alloc(0, submod.exports.len() as u32 + extra_fields)',['node:timers.promises']),
        ('crates/perry-stdlib/src/worker_threads/worker_surface.rs','empty_object','perry_runtime::object::js_object_alloc(0, 0)',['node:worker_threads.resourceLimits','node:worker_threads.default.resourceLimits']),
        (r+'perf_hooks/prototypes.rs','attach_perf_hooks_constructor','crate::object::js_object_alloc(0, 0)',[
            n+'.prototype'for n in ('Performance','PerformanceEntry','PerformanceMark','PerformanceMeasure','PerformanceObserver','PerformanceObserverEntryList','PerformanceResourceTiming')]),
        (r+'object/native_module/namespace_builders.rs','global_agent_prototype','js_object_alloc(0, 0)',[
            'node:http.globalAgent.<prototype>','node:https.globalAgent.<prototype>']),
        (r+'object/native_module/callable_exports/buffer_constructor.rs','buffer_constructor_value','js_object_alloc(0, 0)',['Buffer.prototype']),
        (r+'cluster.rs','ensure_worker_constructor','js_object_alloc(0, 7)',['node:cluster.Worker.prototype']),
        ('crates/perry-stdlib/src/string_decoder.rs','string_decoder_prototype_value','perry_runtime::object::js_object_alloc(0, 7)',['node:string_decoder.StringDecoder.prototype']),
        (r+'object/global_this/populate.rs','populate_global_this_builtins_inner','js_object_alloc(0, 0)',['Intl','Atomics']),
        (r+'object/native_module.rs','js_create_native_module_namespace','js_object_alloc(NATIVE_MODULE_CLASS_ID, 1)',['console']),
        (r+'object/global_this/populate.rs','populate_global_this_builtins_inner','super::proto_room::alloc_builtin_prototype()',[
            d['name']for d in pending.values()if d.get('header')==dict(gc_type=2,reserved=2432,class_id=0)]),
    ]
    for file,function,allocator,names in [
        (r+'navigator.rs','navigator_object_with_constructor','js_object_alloc_with_shape',['navigator']),
        (r+'object/native_module/namespace_builders.rs','http_status_codes_object','js_object_alloc_with_shape',['node:http.STATUS_CODES']),
        (r+'object/global_this/object_intrinsic.rs','build_object_intrinsics','crate::object::layout_desc::alloc_layout_prototype',['Object.prototype']),
        (r+'object/global_this/populate.rs','populate_global_this_builtins_inner','crate::object::layout_desc::alloc_layout_prototype',['Function.prototype']),
        (r+'object/native_module/constructor_shapes.rs','prototype','js_object_alloc_with_shape',['node:http2.Http2ServerRequest.prototype','node:http2.Http2ServerResponse.prototype','node:readline/promises.Interface.prototype.<prototype>', 'node:readline/promises.Interface.prototype.<prototype>.<prototype>', 'node:crypto.Cipheriv.prototype.<prototype>', 'node:crypto.Hash.prototype', 'node:crypto.Hmac.prototype', 'node:repl.REPLServer.prototype.<prototype>', 'node:zlib.BrotliCompress.prototype.<prototype>', 'node:zlib.Deflate.prototype.<prototype>', 'node:zlib.Deflate.prototype.<prototype>.<prototype>', 'node:zlib.ZstdCompress.prototype.<prototype>']),
        (r+'wasi.rs','ensure_wasi_prototype','crate::object::js_object_alloc_with_shape',['node:wasi.WASI.prototype']),
        (r+'tls.rs','ensure_secure_context_prototype','crate::object::js_object_alloc_with_shape',['node:tls.SecureContext.prototype']),
        (r+'process.rs','empty_object_value','crate::object::runtime_layouts::perry_build_runtime_record',['process._events', 'node:process._events']),
        (r+'process/node_module/source_map.rs','source_map_prototype','crate::object::runtime_layouts::perry_alloc_runtime_record',['node:module.SourceMap.prototype']),
        (r+'object/native_module/namespace_builders.rs','http_global_agent_object','js_object_alloc_with_shape',['node:http.globalAgent']),
        (r+'object/native_module/namespace_builders.rs','https_global_agent_object','js_object_alloc_with_shape',['node:https.globalAgent']),
        (r+'object/native_module/callable_exports.rs','ensure_child_process_prototype','js_object_alloc_with_shape',['node:child_process.ChildProcess.prototype']),
        (r+'object/native_module/callable_exports/module_cjs.rs','module_cjs_prototype_value','crate::object::js_object_alloc_with_shape',['node:module.Module.prototype']),
        (r+'process/metadata_values.rs','process_config_value','crate::object::runtime_layouts::perry_build_runtime_record',['process.config.target_defaults.configurations.Release','node:process.config.target_defaults.configurations.Release']),
    ]:
        if not any(n in pending for n in names):continue
        source=(root/file).read_text()
        a,b=expression_in_function(source,function,allocator+'(')
        ts=list(m.tokens(source[a:]))
        end=m.matching(ts,next(i for i,t in enumerate(ts)if t[0]=='('))
        expression=source[a:a+ts[end][2]]
        groups.append((file,function,expression,names))
    edits=collections.defaultdict(list);placements=[]
    for file,function,expression,names in groups:
        ds=[pending[n]for n in names if n in pending]
        if not ds:continue
        source=(root/file).read_text();a,b=expression_in_function(source,function,expression)
        render=(lambda d:m.render_path(d["name"])) if expression.startswith("module_set_value(") else p.render
        if ds[0]['name']=='node:http.STATUS_CODES':
            render=lambda d:m.render_path(d['name'])+', kind = namespace, members = pairs(STATUS_CODES)'
        if expression.startswith('js_object_alloc(NATIVE'):
            from realm_native_object_birth_migration import render
        start,end,replacement,line=addition(source,a,b,[render(d)for d in ds],'perry_runtime'if file.startswith('crates/perry-stdlib/')else'crate')
        edits[file].append((start,end,replacement))
        for d in ds:
            placements.append(dict(name=d['name'],source=file,line=line,old_source='crates/'+oracle[d['name']]['source'],old_line=oracle[d['name']]['line']))
    if apply:
        for file,rs in edits.items():
            path=root/file;source=path.read_text();before=source.count('\n')
            for a,b,replacement in sorted(rs,reverse=True):source=source[:a]+replacement+source[b:]
            assert source.count('\n')==before;path.write_text(source)
        moved={d['name']for d in placements}
        for path,doc in companions.items():
            for group,rows in doc['groups'].items():doc['groups'][group]=[d for d in rows if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                s,n=pattern.subn('',owner.read_text());assert n==1;owner.write_text(s.rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True)
    (output/'migration-inherited-births.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(placements)),indent=2)+'\n')
    print('inherited and intrinsic births moved',len(placements))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
