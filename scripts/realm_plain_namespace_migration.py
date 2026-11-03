#!/usr/bin/env python3
"""Move ordinary namespace members onto the actual allocator expression."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m
import realm_prototype_birth_migration as p

def expression_in_function(source,function,expression):
    ts=list(m.tokens(source))
    starts=[i for i in range(len(ts)-1)if ts[i][0]=='fn'and ts[i+1][0]==function]
    assert len(starts)==1,(function,starts)
    opening=next(i for i in range(starts[0],len(ts))if ts[i][0]=='{')
    end=m.matching(ts,opening)
    matches=[v.start() for v in re.finditer(re.escape(expression),source)if ts[opening][1]<v.start()<ts[end][1]]
    assert len(matches)==1,(function,expression,matches)
    return matches[0],matches[0]+len(expression)

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={f:json.loads(f.read_text())for f in root.glob('crates/*/src/**/*.realm.json')}
    ds={d['name']:d for doc in companions.values()for rows in doc['groups'].values()for d in rows}
    runtime='crates/perry-runtime/src/'
    mod=runtime+'node_submodules/mod.rs'
    groups=[
        (runtime+'navigator.rs','navigator_object_with_constructor','crate::object::js_object_alloc(0, 0)',['navigator.locks']),
        (runtime+'object/global_this_webassembly.rs','install_webassembly_object_property','js_object_alloc(0, 0)',['WebAssembly.JSTag']),
        (runtime+'node_submodules/test.rs','mock_object_value','js_object_alloc(0, 5)',['node:test.mock.timers']),
        (runtime+'node_submodules/test.rs','mock_object_value','js_object_alloc(0, 8)',['node:test.mock']),
        (runtime+'node_submodules/test_snapshot.rs','snapshot_object_value','js_object_alloc(0, 2)',['node:test.snapshot']),
        (mod,'timers_promises_scheduler_value','js_object_alloc(0, 2)',['node:timers/promises.scheduler']),
        (mod,'submodule_default_object_value','js_object_alloc(0, submod.exports.len() as u32 + extra_fields)',[
            'node:diagnostics_channel.default','node:fs.promises','node:stream/consumers.default',
            'node:stream/web.default','node:test/reporters.default','node:timers.default']),
        (mod,'ensure_namespace_singleton','js_object_alloc(0, field_count)',['node:stream.promises']),
        (mod,'ensure_namespace_singleton','js_object_alloc(0, submod.exports.len() as u32)',['node:trace_events.default']),
        (runtime+'object/native_module/callable_exports/module_cjs.rs','module_cjs_extensions_value','js_object_alloc(0, 3)',['node:module._extensions']),
    ]
    for protocol in ('http','https'):
        groups.append((runtime+'object/native_module/namespace_builders.rs',protocol+'_global_agent_object','js_object_alloc(0, 0)',
                       ['node:'+protocol+'.globalAgent.'+key for key in ('freeSockets','requests','sockets')]))
    edits=collections.defaultdict(list);placements=[]
    for file,function,expression,names in groups:
        entries=[ds[n]for n in names if n in ds]
        if not entries:continue
        for d in entries:assert d['header']==dict(gc_type=2,reserved=0,class_id=0)
        source=(root/file).read_text();a,b=expression_in_function(source,function,expression)
        replacement='crate::realm_decl! { birth '+expression+'; realm { '+'; '.join(p.render(d)for d in entries)+'; } }'
        edits[file].append((a,b,replacement))
        for d in entries:
            placements.append(dict(name=d['name'],source=file,line=source[:a].count('\n')+1,old_source='crates/'+oracle[d['name']]['source'],old_line=oracle[d['name']]['line']))
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
    (output/'migration-plain-namespaces.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(placements)),indent=2)+'\n')
    print('ordinary namespaces moved',len(placements))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
