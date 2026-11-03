#!/usr/bin/env python3
"""Move native display-name exceptions to literal projections owned by the body."""
import argparse
import collections
import copy
import json
from pathlib import Path
import realm_source_migration as m
import realm_native_length_migration as lengths

def name_facts(d):
    if not d.get('function_info',{}).get('native_method'):return None
    label=next((p['value']['value']for p in d['properties']if p['key']=='name'),None)
    if label is None or label==m.visible_name(d['name']):return None
    candidate=copy.deepcopy(d);candidate['length']=0
    for p in candidate['properties']:
        if p['key']=='name':p['value']['value']=m.visible_name(d['name'])
        if p['key']=='length':p['value']['value']='0000000000000000'
    facts=m.native_default_facts(candidate)
    return None if facts is None else (facts,label)

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    export='crates/perry-runtime/src/object/native_module/callable_exports.rs'
    shapes='crates/perry-runtime/src/object/native_module/constructor_shapes.rs'
    table='crates/perry-runtime/src/object/native_module/callable_export_arity_table.rs'
    locations=collections.defaultdict(list)
    for file,parse in [(table,lengths.arity_rows),(export,lengths.arity_overrides)]:
        for key,lit in parse((root/file).read_text()).items():locations[key].append((file,lit))
    edits=collections.defaultdict(dict);rows=[];placements=[]
    registry='crates/perry-runtime/src/closure/registry.rs';source=(root/registry).read_text()
    start=source.index('pub static BOUND_METHOD_INFO:');following=source.index('\n',source.index(';',start))+1
    end=source.index('\n',following);offset=source.rindex('}',following,end)
    for doc in companions.values():
        for ds in doc['groups'].values():
            for d in ds:
                result=name_facts(d)
                if result is None:continue
                facts,label=result
                module=oracle[facts['receiver']]['native_namespace'];key=(module,d['captures'][1]['value'])
                name_file=shapes if module=='crypto' and label=='deprecated' else export
                macro='realm_crypto_deprecated' if name_file==shapes else lengths.projection_name(key)
                text=(root/name_file).read_text();needle=json.dumps(label)
                occurrences=[t for t in m.tokens(text)if t[0]==needle]
                assert occurrences,(d['name'],label)
                for lit in occurrences:edits[name_file][lit[1],lit[2]]=macro+'!(name)'
                spec=['name = '+json.dumps(label)]
                if d['length']:
                    ls=locations[key];assert ls,(d['name'],'no arity literal')
                    assert all(int(lit[0])==d['length']for _,lit in ls)
                    for file,lit in ls:edits[file][lit[1],lit[2]]=macro+'!(length)'
                    spec.append('length = '+str(d['length']))
                facts.update(spec='{'+', '.join(spec)+'}',emit='metadata('+macro+')')
                rows.append((d['name'],facts));old=oracle[d['name']]
                placements.append(dict(name=d['name'],source=registry,line=source[:offset].count('\n')+1,old_source='crates/'+old['source'],old_line=old['line']))
    if apply:
        edits[registry][offset,offset]='; '.join(m.render_logical(*r)for r in rows)+'; ' if rows else ''
        for file,rs in edits.items():
            p=root/file;s=p.read_text();before=s.count('\n')
            for (a,b),replacement in sorted(rs.items(),reverse=True):s=s[:a]+replacement+s[b:]
            assert s.count('\n')==before;p.write_text(s)
        moved={r['name']for r in placements}
        for p,doc in companions.items():
            for group,ds in doc['groups'].items():doc['groups'][group]=[d for d in ds if d['name']not in moved]
            assert any(doc['groups'].values());p.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
    report=dict(placements=placements,entries_moved=len(rows),literal_sites=sum(map(len,edits.values())))
    output.mkdir(parents=True,exist_ok=True);(output/'migration-native-names.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
