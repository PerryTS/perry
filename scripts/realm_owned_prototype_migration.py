#!/usr/bin/env python3
"""Migrate shared native and generator prototype births at their allocators."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_prototype_birth_migration as p
import realm_source_migration as m

def eligible_native(d,oracle):
    if d['kind']!='prototype' or not d['name'].startswith('node:') or d['name']=='node:events.EventEmitter.prototype':return False
    if d.get('header')!=dict(gc_type=2,reserved=2176,class_id=0):return False
    ctor=next((v['value']['value']for v in d['properties']if v['key']=='constructor'and'value'in v),None)
    return oracle.get(ctor,{}).get('function_info',{}).get('native_method') is True

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={f:json.loads(f.read_text())for f in root.glob('crates/*/src/**/*.realm.json')}
    ds=[d for doc in companions.values()for entries in doc['groups'].values()for d in entries]
    generator='crates/perry-runtime/src/object/global_this/generator.rs'
    groups=[
        ('crates/perry-runtime/src/object/class_registry/prototype_objects.rs','crate::object::object_alloc_prototype_old(0, 0)',0,[d for d in ds if eligible_native(d,oracle)]),
        (generator,'crate::object::object_alloc_prototype_old(0, 0)',0,[d for d in ds if d['name']=='%AsyncFunctionPrototype%']),
        (generator,'crate::object::object_alloc_prototype_old(0, 0)',1,[d for d in ds if d['name']=='%AsyncIteratorPrototype%']),
        (generator,'super::proto_room::alloc_builtin_prototype_with_room(3)',0,[d for d in ds if d['name']in ('%GeneratorFunctionPrototype%','%AsyncGeneratorFunctionPrototype%')]),
        (generator,'super::proto_room::alloc_builtin_prototype_with_room(if is_async { 5 } else { 6 })',0,[d for d in ds if d['name']in ('%GeneratorPrototype%','%AsyncGeneratorPrototype%')]),
    ]
    edits=collections.defaultdict(list);placements=[]
    for file,expression,index,decls in groups:
        if not decls:continue
        source=(root/file).read_text();matches=[m.start()for m in re.finditer(re.escape(expression),source)]
        offset=matches[index];line=source[:offset].count('\n')+1
        replacement='crate::realm_decl! { birth '+expression+'; realm { '+'; '.join(p.render(d)for d in sorted(decls,key=lambda d:d['name']))+'; } }'
        edits[file].append((offset,offset+len(expression),replacement))
        for d in decls:
            old=oracle[d['name']]
            placements.append(dict(name=d['name'],source=file,line=line,old_source='crates/'+old['source'],old_line=old['line']))
    if apply:
        for file,replacements in edits.items():
            path=root/file;source=path.read_text();before=source.count('\n')
            for a,b,text in sorted(replacements,reverse=True):source=source[:a]+text+source[b:]
            assert before==source.count('\n');path.write_text(source)
        moved={d['name']for d in placements}
        for path,doc in companions.items():
            for group,entries in doc['groups'].items():doc['groups'][group]=[d for d in entries if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                source,n=pattern.subn('',owner.read_text());assert n==1
                owner.write_text(source.rstrip()+'\n')
    report=dict(placements=placements,entries_moved=len(placements),sites=sum(map(len,edits.values())),cross_file_entries=sum(d['source']!=d['old_source']for d in placements))
    output.mkdir(parents=True,exist_ok=True);(output/'migration-owned-prototypes.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
