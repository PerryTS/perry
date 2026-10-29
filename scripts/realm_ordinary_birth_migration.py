#!/usr/bin/env python3
"""Place ordinary prototype and namespace facts at their allocating expressions."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_prototype_birth_migration as p
import realm_source_migration as m

def allocation(source,function,variable):
    ts=list(m.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=='fn'and ts[i+1][0]==function)
    opening=next(i for i in range(start,len(ts))if ts[i][0]=='{')
    end=m.matching(ts,opening)
    matches=[]
    for i in range(opening,end-5):
        if [t[0]for t in ts[i:i+5]]!=['let',variable,'=','js_object_alloc','(']:continue
        closing=m.matching(ts,i+4)
        matches.append((ts[i+3][1],ts[closing][2]))
    if len(matches)!=1:raise ValueError((function,variable,len(matches)))
    return matches[0]

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={f:json.loads(f.read_text())for f in root.glob('crates/*/src/**/*.realm.json')}
    ds=[d for doc in companions.values()for rows in doc['groups'].values()for d in rows if d.get('header')==dict(gc_type=2,reserved=2048,class_id=0)]
    temporal='crates/perry-runtime/src/object/global_this/math_temporal.rs'
    wasm='crates/perry-runtime/src/object/global_this_webassembly.rs'
    groups=[
        ('crates/perry-runtime/src/intl/install.rs','install_constructor','proto',[d for d in ds if d['name'].startswith('Intl.')and d['name'].endswith('.prototype')]),
        (temporal,'install_temporal_prototype','proto',[d for d in ds if d['name'].startswith('Temporal.')and d['name'].endswith('.prototype')and d['name']!='Temporal.ZonedDateTime.prototype']),
        (temporal,'build_zoned_date_time_prototype','proto',[d for d in ds if d['name']=='Temporal.ZonedDateTime.prototype']),
        (temporal,'build_temporal_now_namespace','now_obj',[d for d in ds if d['name']=='Temporal.Now']),
        (wasm,'install_webassembly_constructor','proto_obj',[d for d in ds if d['name'].startswith('WebAssembly.')and d['name'].endswith('.prototype')]),
        (wasm,'create_webassembly_namespace','ns_obj',[d for d in ds if d['name']=='WebAssembly']),
    ]
    native='crates/perry-runtime/src/object/native_module/'
    for function,variable,names in [
        ('attach_assert_prototype','proto',['node:assert.Assert.prototype']),
        ('attach_crypto_key_object_shape','proto',['node:crypto.KeyObject.prototype']),
        ('attach_crypto_x509_certificate_shape','proto',['node:crypto.X509Certificate.prototype']),
        ('attach_tls_constructor_prototype','prototype',['node:tls.TLSSocket.prototype']),
    ]:
        groups.append((native+'callable_exports.rs',function,variable,[d for d in ds if d['name']in names]))
    groups.append((native+'async_hooks_exports.rs','attach_prototype','prototype',[d for d in ds if d['name']in ('node:async_hooks.AsyncLocalStorage.prototype','node:async_hooks.AsyncResource.prototype')]))
    # The generic global namespace branch owns Temporal itself.
    if any(d['name']=='Temporal' for d in ds):
        populate=root/'crates/perry-runtime/src/object/global_this/populate.rs'
        source=populate.read_text()
        ts=list(m.tokens(source));start=source.index('let ns_obj = js_object_alloc(0, 0);')
        fn=next(t[0]for i,t in reversed(list(enumerate(ts)))if t[1]<start and i>0 and ts[i-1][0]=='fn')
        groups.append((str(populate.relative_to(root)),fn,'ns_obj',[d for d in ds if d['name']=='Temporal']))
    edits=collections.defaultdict(list);placements=[]
    for file,function,variable,decls in groups:
        if not decls:continue
        source=(root/file).read_text();a,b=allocation(source,function,variable)
        expression=source[a:b]
        replacement='crate::realm_decl! { birth '+expression+'; realm { '+'; '.join(p.render(d)for d in sorted(decls,key=lambda d:d['name']))+'; } }'
        edits[file].append((a,b,replacement))
        for d in decls:
            old=oracle[d['name']]
            placements.append(dict(name=d['name'],source=file,line=source[:a].count('\n')+1,old_source='crates/'+old['source'],old_line=old['line']))
    if apply:
        for file,rs in edits.items():
            path=root/file;source=path.read_text();before=source.count('\n')
            for a,b,text in sorted(rs,reverse=True):source=source[:a]+text+source[b:]
            assert source.count('\n')==before;path.write_text(source)
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
    output.mkdir(parents=True,exist_ok=True);(output/'migration-ordinary-births.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
