#!/usr/bin/env python3
"""Expose each HTTP macro-generated body's existing static info at its invocation."""
import argparse
import copy
import json
import re
from pathlib import Path
import realm_source_migration as m
from realm_metadata_migration import human_length

def rewrite(source,declarations):
    old='($body:ident, $op:ident, $info:ident, $min:expr, $server_only:expr) => {'
    assert source.count(old)==1
    source=source.replace(old,'($body:ident, $op:ident, $info:ident, $min:expr, $server_only:expr, $initializer:expr) => {')
    old='static $info: JsFunctionInfo = JsFunctionInfo::of($body as JsBody3).with_declared(3);'
    assert source.count(old)==1
    source=source.replace(old,'static $info: JsFunctionInfo = $initializer;')
    placements=[]
    for d in declarations:
        body=d['body'].rsplit('::',1)[1]
        pattern=re.compile(r'method!\('+re.escape(body)+r', (\w+), (\w+), (\d+), (true|false)\);')
        matches=list(pattern.finditer(source));assert len(matches)==1,(body,len(matches))
        match=matches[0]
        facts=m.simple_facts(d)
        if facts is None:
            facts,length=human_length(d)
            facts.update(spec='{length = '+str(length)+'}',emit='metadata(realm_http_set_header_public)')
        initializer='perry_ffi::realm_decl! { info_expr JsFunctionInfo::of('+body+' as JsBody3).with_declared(3); realm { '+m.render(d['name'],facts)+'; } }'
        new='#[rustfmt::skip] '+match[0][:-2]+', '+initializer+');'
        placements.append(dict(name=d['name'],line=source[:match.start()].count('\n')+1))
        source=source[:match.start()]+new+source[match.end():]
    return source,placements

def migrate(root,baseline,output,apply=False):
    m.CONSTANTS=m.abi_constants(root);m.FAMILIES=m.constructor_families(root)
    oracle={d['name']:d for d in baseline['declarations']}
    companions={f:json.loads(f.read_text())for f in root.glob('crates/*/src/**/*.realm.json')}
    ds=[d for doc in companions.values()for rows in doc['groups'].values()for d in rows if d.get('body','').startswith('perry_ext_http::server::response_methods::')]
    assert len(ds)==4
    file='crates/perry-ext-http/src/server/response_methods.rs';path=root/file;before=path.read_text()
    source,placements=rewrite(before,ds);assert source.count('\n')==before.count('\n')
    for p in placements:p.update(source=file,old_source='crates/'+oracle[p['name']]['source'],old_line=oracle[p['name']]['line'])
    table=root/'crates/perry-runtime/src/object/node_http_prototypes.rs';text=table.read_text()
    assert text.count('("setHeader", 2),')==1
    if apply:
        path.write_text(source);table.write_text(text.replace('("setHeader", 2),','("setHeader", realm_http_set_header_public!(length)),'))
        moved={d['name']for d in ds}
        for path,doc in companions.items():
            for group,rows in doc['groups'].items():doc['groups'][group]=[d for d in rows if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                text,count=pattern.subn('',owner.read_text());assert count==1
                owner.write_text(text.rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True)
    (output/'migration-http-info.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(ds)),indent=2)+'\n')
    print('HTTP info declarations moved',len(ds))
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
