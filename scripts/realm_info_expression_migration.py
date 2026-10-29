#!/usr/bin/env python3
"""Migration record for typed info expressions in existing runtime birth tables."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m
import realm_named_info_migration as named

def expressions(source):
    ts=list(m.tokens(source));excluded=m.test_ranges(ts);sites=[]
    for i,t in enumerate(ts[:-3]):
        if t[0]=='macro_rules' and ts[i+1][0]=='!' and ts[i+3][0]in ('{','('):
            end=m.matching(ts,i+3);excluded.append((t[1],ts[end][2]))
        elif t[0]=='realm_decl' and ts[i+1][0]=='!' and ts[i+2][0]=='{':
            end=m.matching(ts,i+2);excluded.append((t[1],ts[end][2]))

    for i,t in enumerate(ts):
        if t[0]!='JsFunctionInfo' or i+3>=len(ts) or ts[i+1][0]!='::' or ts[i+2][0]not in ('of','of_native_args')or ts[i+3][0]!='(':continue
        if any(a<=t[1]<b for a,b in excluded):continue
        close=m.matching(ts,i+3);at=close+1
        while at+2<len(ts) and ts[at][0]=='.' and ts[at+2][0]=='(':
            at=m.matching(ts,at+2)+1
        # Keep preceding qualification. Wrapping an already-owned initializer
        # or a macro template would give the ABI two declaration owners.
        begin=i
        while begin>=2 and ts[begin-1][0]=='::' and re.fullmatch(r'[A-Za-z_]\w*',ts[begin-2][0]):begin-=2
        if '$'in source[ts[begin][1]:ts[at-1][2]]:continue
        expr=source[ts[begin][1]:ts[at-1][2]]
        parsed=named.named_infos('static X: JsFunctionInfo = '+expr+';')
        if len(parsed)!=1:continue
        site=parsed[0];site.update(offset=ts[begin][1],end=ts[at-1][2],line=source[:ts[begin][1]].count('\n')+1)
        sites.append(site)
    return sites

def migrate(root,baseline,output,apply=False):
    m.CONSTANTS=m.abi_constants(root);m.FAMILIES=m.constructor_families(root)
    oracle={d['name']:d for d in baseline['declarations']}
    sites=collections.defaultdict(list)
    for path in root.glob('crates/*/src/**/*.rs'):
        if 'tests'in path.parts or path.stem.endswith('_tests'):continue
        source=path.read_text()
        if 'JsFunctionInfo::of'not in source:continue
        for s in expressions(source):
            s['path']=str(path.relative_to(root));sites[s['body'].split('::')[-1]].append(s)
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    groups=collections.defaultdict(list);placements=[]
    for doc in companions.values():
        for ds in doc['groups'].values():
            for d in ds:
                facts=m.simple_facts(d)
                if facts is None:continue
                matches=[s for s in sites[d['body'].split('::')[-1]]if all(d['function_info'].get(k)==v for k,v in s['signature'].items())]
                if not matches:continue
                old=oracle[d['name']]
                s=min(matches,key=lambda s:m.placement_rank(s,d['body'],'crates/'+old['source']))
                groups[(s['path'],s['offset'],s['end'])].append((d['name'],facts))
                placements.append(dict(name=d['name'],source=s['path'],line=s['line'],old_source='crates/'+old['source'],old_line=old['line']))
    if apply:
        edits=collections.defaultdict(list)
        for (file,start,end),ds in groups.items():
            edits[file]+=[(start,'crate::realm_decl! { info_expr '),(end,'; realm { '+'; '.join(m.render(*d)for d in sorted(ds))+'; } }')]
        for file,es in edits.items():
            p=root/file;p.write_text(m.insert_at_sites(p.read_text(),es))
        moved={p['name']for p in placements}
        for p,doc in companions.items():
            for key,ds in doc['groups'].items():doc['groups'][key]=[d for d in ds if d['name']not in moved]
            if any(doc['groups'].values()):p.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                p.unlink();owner=p.with_name(p.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(p.name)+r'#\*"\);[^\n]*$',re.M)
                s,n=pattern.subn('',owner.read_text());assert n==1
                owner.write_text(s.rstrip()+'\n')
    report=dict(placements=placements,entries_moved=len(placements),sites=len(groups),cross_file_entries=sum(p['source']!=p['old_source']for p in placements))
    output.mkdir(parents=True,exist_ok=True);(output/'migration-info-expressions.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1])
    p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
