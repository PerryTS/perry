#!/usr/bin/env python3
"""Declare internal constructor cells where their existing parent keys are born."""
import argparse
import collections
import json
from pathlib import Path
import realm_source_migration as m

def parent_keys(source):
    ts=list(m.tokens(source));result={}
    begin=next(i for i in range(len(ts)-1)if ts[i][0]=='fn'and ts[i+1][0]=='native_parent')
    opening=next(i for i in range(begin,len(ts))if ts[i][0]=='{');end=m.matching(ts,opening)
    for i in range(opening,end-7):
        if ts[i][0]!='=>'or ts[i+1][0]!='(':continue
        close=m.matching(ts,i+1);args=m.split(ts[i+2:close])
        if len(args)!=2 or any(len(a)!=1 or not a[0][0].startswith('"')for a in args):continue
        result[tuple(json.loads(a[0][0])for a in args)]=args[1][0]
    return result

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    file='crates/perry-runtime/src/object/native_module/constructor_shapes.rs';source=(root/file).read_text()
    keys=parent_keys(source);groups=collections.defaultdict(list);placements=[]
    for doc in companions.values():
        for ds in doc['groups'].values():
            for d in ds:
                if not d.get('function_info',{}).get('native_method'):continue
                captures=d['captures']
                if len(captures)!=3 or captures[1]['kind']!='bytes' or d['length']!=0:continue
                key=captures[1]['value'];module=oracle[captures[0]['value']]['native_namespace']
                if not (key.startswith('#') or d['kind']=='prototype') or (module,key)not in keys:continue
                prop=next(p for p in d['properties']if p['key']=='prototype')
                assert prop['attributes']==1 and prop['value']['kind']=='reference'
                instance=prop['value']['value']
                facts=dict(receiver=captures[0]['value'],kind=d['kind'],instance=instance)
                if d['prototype']!='Function.prototype':facts['extends']=d['prototype']
                implied={instance,d['prototype'],captures[0]['value']}
                extra=[b for b in d['births']if b not in implied]
                if extra:facts['births']=extra
                assert len(d['properties'])==3
                groups[module,key].append((d['name'],facts))
                lit=keys[module,key];old=oracle[d['name']]
                placements.append(dict(name=d['name'],source=file,line=source[:lit[1]].count('\n')+1,old_source='crates/'+old['source'],old_line=old['line']))
    if apply:
        for key,rows in sorted(groups.items(),key=lambda kv:keys[kv[0]][1],reverse=True):
            lit=keys[key]
            replacement='crate::realm_decl! { native_key '+lit[0]+'; realm { '+'; '.join(m.render_logical(*r)for r in rows)+'; } }'
            source=source[:lit[1]]+replacement+source[lit[2]:]
        (root/file).write_text(source)
        moved={r['name']for r in placements}
        for p,doc in companions.items():
            for group,ds in doc['groups'].items():doc['groups'][group]=[d for d in ds if d['name']not in moved]
            assert any(doc['groups'].values());p.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
    report=dict(placements=placements,entries_moved=len(placements),sites=len(groups))
    output.mkdir(parents=True,exist_ok=True);(output/'migration-native-keys.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
