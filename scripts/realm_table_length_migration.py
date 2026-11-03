#!/usr/bin/env python3
"""Migration record: literal method lengths consumed by shared noop births."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m
from realm_metadata_migration import human_length


def table_lengths(source):
    ts=list(m.tokens(source))
    literals={}
    for i,t in enumerate(ts[:-1]):
        if t[0]!="install_noop_proto_methods" or ts[i+1][0]!="(" or (i and ts[i-1][0]=="fn"):continue
        end=m.matching(ts,i+1)
        args=m.split(ts[i+2:end])
        if len(args)!=2:continue
        rows=args[1]
        if len(rows)==1:
            starts=[j for j in range(len(ts)-1)if ts[j][0]=="const"and ts[j+1][0]==rows[0][0]]
            if len(starts)!=1:continue
            equals=next(j for j in range(starts[0]+2,len(ts))if ts[j][0]=="=")
            rows=m.split(ts[equals+1:],";")[0]
        if m.joined(rows[:2])!="&[" or rows[-1][0]!="]":continue
        for row in m.split(rows[2:-1]):
            if row[0][0]!="(" or row[-1][0]!=")":continue
            args=m.split(row[1:-1])
            if len(args)!=2 or any(len(a)!=1 for a in args):continue
            key,length=args[0][0],args[1][0]
            if not key[0].startswith('"') or not length[0].isdigit():continue
            literals[length[1]]=(json.loads(key[0]),length)
    return list(literals.values())


def migrate(root,baseline,output,apply=False):
    m.CONSTANTS=m.abi_constants(root);m.FAMILIES=m.constructor_families(root)
    oracle={d["name"]:d for d in baseline["declarations"]}
    sites=m.scan(root)
    literals=collections.defaultdict(list)
    for path in root.glob("crates/perry-runtime/src/**/*.rs"):
        source=path.read_text()
        if "install_noop_proto_methods"not in source:continue
        for name,literal in table_lengths(source):
            literals[(name,int(literal[0]))].append((str(path.relative_to(root)),literal))
    companions={p:json.loads(p.read_text())for p in root.glob("crates/*/src/**/*.realm.json")}
    groups=collections.defaultdict(list);placements=[];unplaced=[]
    for path,doc in companions.items():
        for entries in doc["groups"].values():
            for entry in entries:
                if entry.get("body")!="global_this_builtin_noop_thunk":continue
                result=human_length(entry)
                if result is None:continue
                facts,length=result
                key=m.visible_name(entry["name"])
                if not literals[(key,length)]:unplaced.append(entry["name"]);continue
                candidates=[s for s in sites[entry["body"].split("::")[-1]]if all(entry["function_info"].get(k)==v for k,v in s["signature"].items())]
                if not candidates:unplaced.append(entry["name"]);continue
                site=min(candidates,key=lambda s:m.placement_rank(s,entry["body"],"crates/"+oracle[entry["name"]]["source"]))
                macro="realm_noop_"+re.sub(r"\W+","_",key.lower())+"_"+str(length)+"_metadata"
                groups[(site["path"],site["offset"])].append((entry,facts,site,macro))
                placements.append(dict(name=entry["name"],source=site["path"],line=site["line"],
                    old_source="crates/"+oracle[entry["name"]]["source"],projection=macro,
                    literal_sites=[dict(source=file,line=(root/file).read_text()[:lit[1]].count("\n")+1)for file,lit in literals[(key,length)]]))
    edits=collections.defaultdict(dict)
    for (file,offset),entries in groups.items():
        entries.sort(key=lambda row:row[0]["name"])
        site=entries[0][2];annotation=site["annotation"]
        rendered=list(annotation["facts"])if annotation else []
        for entry,facts,_,macro in entries:
            rendered.append(m.render(entry["name"],facts)+f", spec = {{length = {entry['length']}}}, emit = metadata({macro})")
            key=m.visible_name(entry["name"])
            for target,literal in literals[(key,entry["length"])]:
                edits[target][(literal[1],literal[2])]=macro+"!(length)"
        inner=rendered[0]if len(rendered)==1 else "realm_decl! { "+"; ".join(rendered)+"; }"
        start,end=(annotation["start"],annotation["end"])if annotation else (offset,offset)
        edits[file][(start,end)]="#[realm("+inner+")]"+(""if annotation else " ")
    moved={p["name"]for p in placements};deleted=[]
    if apply:
        for file,replacements in edits.items():
            path=root/file;source=path.read_text();before=source.count("\n")
            for (start,end),value in sorted(replacements.items(),reverse=True):source=source[:start]+value+source[end:]
            assert source.count("\n")==before
            path.write_text(source)
        for path,doc in companions.items():
            for group,entries in doc["groups"].items():doc["groups"][group]=[d for d in entries if d["name"]not in moved]
            if any(doc["groups"].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+"\n")
            else:
                deleted.append(str(path.relative_to(root)));path.unlink()
                owner=path.with_name(path.name.replace(".realm.json",".rs"))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                source,count=pattern.subn("",owner.read_text());assert count==1,(path,count)
                owner.write_text(source.rstrip()+"\n")
    report=dict(placements=placements,unplaced=unplaced,deleted_companions=deleted,
        entries_moved=len(placements),annotated_sites=len(groups),files_changed=len(edits),
        cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements))
    output.mkdir(parents=True,exist_ok=True)
    (output/"migration-table-length.json").write_text(json.dumps(report,indent=2)+"\n")
    print({k:v for k,v in report.items()if k not in ("placements","unplaced","deleted_companions")})
    print("unplaced",len(unplaced),"companions deleted",len(deleted))

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline",type=Path)
    parser.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--apply",action="store_true")
    args=parser.parse_args()
    migrate(args.root,json.loads(args.baseline.read_text()),args.output,args.apply)
