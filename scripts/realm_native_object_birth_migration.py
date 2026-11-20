#!/usr/bin/env python3
"""Place native namespace objects at their existing allocator birth."""
import argparse
import json
import re
from pathlib import Path
import realm_source_migration as m
import realm_prototype_birth_migration as p

def render(d):
    rows,edges=p.members(d,reference_attrs=7,constant_attrs=7)
    edges.add(d['prototype'])
    fields=[m.render_path(d['name'])]
    if d['prototype']!='Object.prototype':fields.append('extends = '+m.render_path(d['prototype']))
    fields.append('members = { '+'; '.join(rows)+('; ' if rows else '')+'}')
    extra=set(d['births'])-edges
    if extra:fields.append('births = ['+', '.join(m.render_path(v)for v in sorted(extra))+']')
    return ', '.join(fields)

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    allowed={'name','kind','properties','own_key_order','own_symbol_order','header','prototype','births'}
    ds=[d for doc in companions.values()for rows in doc['groups'].values()for d in rows if d.get('header')==dict(class_id=0xfffffffe,gc_type=2,reserved=0) and set(d)==allowed]
    file='crates/perry-runtime/src/object/native_module.rs';path=root/file;source=path.read_text()
    expression='js_object_alloc(NATIVE_MODULE_CLASS_ID, 1)';assert source.count(expression)==1
    at=source.index(expression);line=source[:at].count('\n')+1
    placements=[dict(name=d['name'],source=file,line=line,old_source='crates/'+oracle[d['name']]['source'],old_line=oracle[d['name']]['line'])for d in ds]
    if apply:
        replacement='crate::realm_decl! { birth '+expression+'; realm { '+'; '.join(render(d)for d in sorted(ds,key=lambda d:d['name']))+'; } }'
        path.write_text(source[:at]+replacement+source[at+len(expression):])
        moved={r['name']for r in placements}
        for path,doc in companions.items():
            for key,rows in doc['groups'].items():doc['groups'][key]=[d for d in rows if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                s,n=pattern.subn('',owner.read_text());assert n==1
                owner.write_text(s.rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True);(output/'migration-native-object-births.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(ds)),indent=2)+'\n')
    print('native object births moved',len(ds))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
