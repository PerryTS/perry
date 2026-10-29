#!/usr/bin/env python3
"""Place ESM export facts at the existing namespace-layout allocation."""
import argparse
import copy
import json
import re
from pathlib import Path
import realm_source_migration as m
import realm_prototype_birth_migration as p

def render(d):
    candidate=copy.deepcopy(d)
    candidate['properties']=[v for v in candidate['properties']if not v['symbol']]
    candidate['own_symbol_order']=[]
    rows,edges=p.members(candidate)
    suffix=', attrs = writable | enumerable'
    assert all(row.endswith(suffix)for row in rows)
    rows=[row.removesuffix(suffix)for row in rows]
    result=m.render_path(d['name'])+', members = { '+'; '.join(rows)+'; }'
    extra=set(d['births'])-edges
    if extra:result+=', births = ['+', '.join(m.render_path(v)for v in sorted(extra))+']'
    return result

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    ds=[d for doc in companions.values()for rows in doc['groups'].values()for d in rows if d.get('header',{}).get('class_id')==0xffff4e53 and 'body'not in d]
    file='crates/perry-runtime/src/object/namespace_create.rs';path=root/file;source=path.read_text()
    expression='super::layout_desc::perry_alloc_layout(desc)';assert source.count(expression)==1
    at=source.index(expression);line=source[:at].count('\n')+1
    placements=[dict(name=d['name'],source=file,line=line,old_source='crates/'+oracle[d['name']]['source'],old_line=oracle[d['name']]['line'])for d in ds]
    if apply:
        replacement='crate::realm_decl! { birth '+expression+'; realm { '+'; '.join(render(d)for d in sorted(ds,key=lambda d:d['name']))+'; } }'
        if 'crate::realm_decl! { birth '+expression+'; realm {' in source:
            ts=list(m.tokens(source));start=next(i for i,t in enumerate(ts)if t[0]=='realm_decl'and t[1]<at and source[t[1]:at].startswith('realm_decl! { birth '))
            end=m.matching(ts,start+2)
            realm_close=ts[end-1][1]
            source=source[:realm_close]+'; '.join(render(d)for d in sorted(ds,key=lambda d:d['name']))+'; '+source[realm_close:]
            path.write_text(source)
        else:path.write_text(source[:at]+replacement+source[at+len(expression):])
        moved={r['name']for r in placements}
        for path,doc in companions.items():
            for key,rows in doc['groups'].items():doc['groups'][key]=[d for d in rows if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                s,n=pattern.subn('',owner.read_text());assert n==1
                owner.write_text(s.rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True);(output/'migration-module-namespaces.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(ds)),indent=2)+'\n')
    print('module namespaces moved',len(ds))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
