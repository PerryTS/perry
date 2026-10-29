#!/usr/bin/env python3
"""Migration cleanup: remove constructor edges already implied by its birth."""
import argparse
import collections
import json
from pathlib import Path
import realm_source_migration as m

def path(ts):
    return json.loads(ts[0][0])if len(ts)==1 and ts[0][0].startswith('"')else m.joined(ts)

def clean(text):
    ts=list(m.tokens(text));clauses=m.split(ts)
    fields={row[0][0]:row[2:]for row in clauses[1:]if len(row)>2 and row[1][0]=='='}
    if m.joined(fields.get('kind',[]))!='constructor' or'births'not in fields:return text
    name=path(clauses[0])
    implied={path(fields['extends'])if'extends'in fields else 'Function.prototype',path(fields['instance'])if'instance'in fields else name+'.prototype'}
    births=fields['births']
    paths=m.split(births[1:-1]);remaining=[p for p in paths if path(p)not in implied]
    if len(paths)==len(remaining):return text
    parts=[]
    for row in clauses:
        if row[0][0]=='births':
            if remaining:parts.append('births = ['+', '.join(m.render_path(path(p))for p in remaining)+']')
        else:parts.append(text[row[0][1]:row[-1][2]])
    return ', '.join(parts)

def migrate(root,output,apply=False):
    m.CONSTANTS=m.abi_constants(root)
    sites=m.scan(root);edits=collections.defaultdict(list);report=[]
    for rows in sites.values():
        for site in rows:
            a=site['annotation']
            if not a:continue
            facts=[clean(f)for f in a['facts']]
            if facts==a['facts']:continue
            inner=facts[0]if len(facts)==1 else'realm_decl! { '+'; '.join(facts)+'; }'
            edits[site['path']].append((a['start'],a['end'],'#[realm('+inner+')]'))
            report.append(dict(source=site['path'],line=site['line'],entries=sum(a!=b for a,b in zip(facts,a['facts']))))
    if apply:
        for file,rs in edits.items():
            p=root/file;s=p.read_text();before=s.count('\n')
            for a,b,text in sorted(rs,reverse=True):s=s[:a]+text+s[b:]
            assert before==s.count('\n');p.write_text(s)
    output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(report,indent=2)+'\n')
    print('removed derived constructor edges from',sum(r['entries']for r in report),'entries at',len(report),'sites')
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,a.output,a.apply)
