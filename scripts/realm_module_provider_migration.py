#!/usr/bin/env python3
"""Own submodule provider functions together with their module export facts."""
import argparse
import json
import re
from pathlib import Path
import realm_source_migration as m
from realm_module_namespace_migration import render

def provider_item(source,name):
    ts=list(m.tokens(source))
    fn=next(i for i in range(len(ts)-1)if ts[i][0]=='fn'and ts[i+1][0]==name)
    attribute=source.rfind('#[no_mangle]',0,ts[fn][1])
    if attribute<0:raise ValueError('provider needs its existing exported function')
    while attribute>0:
        end=attribute-1;start=source.rfind('\n',0,end)+1
        if not source[start:end].lstrip().startswith('///'):break
        attribute=start
    opening=next(i for i in range(fn,len(ts))if ts[i][0]=='{')
    end=m.matching(ts,opening)
    return attribute,ts[end][2]

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    ds=[d for doc in companions.values()for rows in doc['groups'].values()for d in rows if d.get('header',{}).get('class_id')==0xffff4e53 and 'body'in d]
    assert {d['body']for d in ds}=={'crate::node_submodules::js_node_submodule_namespace'}
    file='crates/perry-runtime/src/node_submodules/mod.rs';path=root/file;source=path.read_text()
    a,b=provider_item(source,'js_node_submodule_namespace')
    placements=[dict(name=d['name'],source=file,line=source[:a].count('\n')+1,old_source='crates/'+oracle[d['name']]['source'],old_line=oracle[d['name']]['line'])for d in ds]
    if apply:
        replacement='crate::realm_decl! { module_provider '+source[a:b]+' realm { '+'; '.join(render(d)for d in sorted(ds,key=lambda d:d['name']))+'; } }'
        assert replacement.count('\n')==source[a:b].count('\n');path.write_text(source[:a]+replacement+source[b:])
        moved={r['name']for r in placements}
        for path,doc in companions.items():
            for key,rows in doc['groups'].items():doc['groups'][key]=[d for d in rows if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                s,n=pattern.subn('',owner.read_text());assert n==1
                owner.write_text(s.rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True);(output/'migration-module-providers.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(ds)),indent=2)+'\n')
    print('module providers moved',len(ds))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
