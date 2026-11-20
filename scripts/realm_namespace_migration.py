#!/usr/bin/env python3
"""Migration record: bind namespace realm births to their existing Member rows."""
import argparse
import collections
import json
import re
from pathlib import Path
from realm_source_migration import tokens, joined, matching, split, insert_at_sites, render_path


def namespace_factories(source):
    ts=list(tokens(source))
    tables={}
    for i,t in enumerate(ts):
        if t[0]!="const" or i+2>=len(ts):continue
        end=next((j for j in range(i+2,len(ts))if ts[j][0]in ("=",";")),None)
        if end is None or ts[end][0]!="=" or joined(ts[end+1:end+3])!="&[":continue
        close=matching(ts,end+2)
        tag=[]
        for row in split(ts[end+3:close]):
            if joined(row[:4])=="Member::tag(" and len(row)==6 and row[4][0].startswith('"'):
                tag.append(json.loads(row[4][0]))
        if len(tag)==1:tables[ts[i+1][0]]=tag[0]
    factories=[]
    for i in range(len(ts)-5):
        if joined(ts[i:i+4])!="builtin_namespace::create(":continue
        table=ts[i+4][0]
        if ts[i+5][0]!=")" or table not in tables:continue
        fn=next((j for j in range(i-1,-1,-1)if ts[j][0]=="fn"),None)
        if fn is None:continue
        blanks=[m for m in re.finditer(r"(?m)^ *$",source[:ts[fn][1]]) if m.end()<ts[fn][1]]
        if not blanks:raise ValueError("namespace birth has no line-preserving declaration slot")
        slot=blanks[-1].start()
        factories.append(dict(name=tables[table],table=table,offset=slot,
                              line=source[:slot].count("\n")+1,factory=ts[fn+1][0]))
    return factories


def migrate(root, baseline, output, apply=False):
    oracle={d["name"]:d for d in baseline["declarations"]}
    companions={p:json.loads(p.read_text())for p in root.glob("crates/*/src/**/*.realm.json")}
    pending={d["name"]for doc in companions.values()for group in doc["groups"].values()for d in group}
    edits=collections.defaultdict(list)
    placements=[]
    for path in root.glob("crates/*/src/**/*.rs"):
        if "tests" in path.parts or path.stem.endswith("_tests"):continue
        source=path.read_text()
        if "builtin_namespace::create("not in source:continue
        for factory in namespace_factories(source):
            name=factory["name"]
            if name not in pending:continue
            d=oracle[name]
            if d["kind"]!="namespace":continue
            implied={"Object.prototype"}
            for p in d["properties"]:
                value=p.get("value",{})
                if value.get("kind")=="reference":implied.add(value["value"])
            extra=sorted(set(d["births"])-implied)
            declaration=f"crate::realm_decl! {{ {render_path(name)}, provider = namespace({factory['table']})"
            if extra:declaration+=", births = ["+", ".join(map(render_path,extra))+"]"
            declaration+="; }"
            edits[path].append((factory["offset"],declaration))
            placements.append(dict(name=name,source=str(path.relative_to(root)),line=factory["line"],
                                   old_source="crates/"+d["source"],old_line=d["line"],
                                   factory=factory["factory"],member_binding=factory["table"]))
    moved={p["name"]for p in placements}
    assert len(moved)==len(placements),"ambiguous namespace births"
    deleted=[]
    if apply:
        for path,insertions in edits.items():path.write_text(insert_at_sites(path.read_text(),insertions))
        for path,doc in companions.items():
            for group,entries in doc["groups"].items():doc["groups"][group]=[d for d in entries if d["name"]not in moved]
            if any(doc["groups"].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+"\n")
            else:
                deleted.append(str(path.relative_to(root)))
                path.unlink()
                owner=path.with_name(path.name.replace(".realm.json",".rs"))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                source,count=pattern.subn("",owner.read_text())
                assert count==1,(path,count)
                owner.write_text(source.rstrip()+"\n")
    output.mkdir(parents=True,exist_ok=True)
    report=dict(placements=placements,entries_moved=len(placements),deleted_companions=deleted,
                cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements))
    (output/"migration-namespaces.json").write_text(json.dumps(report,indent=2)+"\n")
    print("namespace births",len(placements),"companions deleted",len(deleted))


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline",type=Path)
    parser.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--apply",action="store_true")
    args=parser.parse_args()
    migrate(args.root,json.loads(args.baseline.read_text()),args.output,args.apply)
