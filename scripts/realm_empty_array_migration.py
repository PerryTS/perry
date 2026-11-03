#!/usr/bin/env python3
"""Declare fixed empty process arrays at their existing literal factory calls."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m

def field_keys(source,layout):
    atoms=dict(re.findall(r'runtime_atom!\(\s*(?:reuse\s+)?(\w+)\s*,\s*("(?:\\.|[^"\\])*"),',source))
    match=re.search(r'runtime_layout!\('+layout+r',.*?\[([^\]]*)\]',source)
    if not match:raise ValueError('missing record layout')
    return [json.loads(atoms[a.strip()])for a in match[1].split(',')if a.strip()]

def sites(root):
    result=[]
    file='crates/perry-runtime/src/process/metadata_values.rs';source=(root/file).read_text();ts=list(m.tokens(source))
    keys=field_keys((root/'crates/perry-runtime/src/object/runtime_layouts/records.rs').read_text(),'RECORD_Q468_2')
    for i in range(len(ts)-4):
        if m.joined(ts[i:i+4])!='target_defaults.set(':continue
        end=m.matching(ts,i+3);args=m.split(ts[i+4:end])
        if len(args)!=2 or len(args[0])!=1 or not args[0][0][0].isdigit():continue
        values=args[1]
        for j,t in enumerate(values):
            if t[0]=='module_array_value'and m.joined(values[j:j+6])=='module_array_value(&[])':
                result.append((file,t[1],values[j+5][2],'process.config.target_defaults.'+keys[int(args[0][0][0])]))
    file='crates/perry-runtime/src/process.rs';source=(root/file).read_text()
    for match in re.finditer(r'"(moduleLoadList|_preload_modules)" => (module_array_value\(&\[\]\))',source):
        result.append((file,match.start(2),match.end(2),'process.'+match[1]))
    return result

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    pending={d['name']:d for doc in companions.values()for entries in doc['groups'].values()for d in entries}
    edits=collections.defaultdict(list);placements=[]
    for file,a,b,name in sites(root):
        names=[n for n in (name,'node:'+name)if n in pending]
        if not names:continue
        source=(root/file).read_text();expr=source[a:b]
        assert expr=='module_array_value(&[])'
        text='crate::realm_decl! { birth '+expr+'; realm { '+'; '.join(m.render_path(n)for n in names)+'; } }'
        edits[file].append((a,b,text))
        for name in names:
            old=oracle[name]
            placements.append(dict(name=name,source=file,line=source[:a].count('\n')+1,old_source='crates/'+old['source'],old_line=old['line']))
    if apply:
        for file,rs in edits.items():
            path=root/file;source=path.read_text()
            for a,b,text in sorted(rs,reverse=True):source=source[:a]+text+source[b:]
            path.write_text(source)
        moved={p['name']for p in placements}
        for path,doc in companions.items():
            for group,entries in doc['groups'].items():doc['groups'][group]=[d for d in entries if d['name']not in moved]
            assert any(doc['groups'].values()),path
            path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
    report=dict(placements=placements,entries_moved=len(placements),sites=sum(map(len,edits.values())),cross_file_entries=sum(p['source']!=p['old_source']for p in placements))
    output.mkdir(parents=True,exist_ok=True);(output/'migration-empty-arrays.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
