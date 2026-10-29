#!/usr/bin/env python3
"""Migration record: native export lengths move to their shared body declaration."""
import argparse
import collections
import copy
import json
import re
from pathlib import Path
import realm_source_migration as m


def arity_rows(source):
    ts=list(m.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=="static"and ts[i+1][0]=="CALLABLE_EXPORT_ARITY_TABLE")
    equals=next(i for i in range(start,len(ts))if ts[i][0]=="=")
    assert m.joined(ts[equals+1:equals+3])=="&["
    end=m.matching(ts,equals+2)
    rows={}
    for row in m.split(ts[equals+3:end]):
        assert row[0][0]=="("and row[-1][0]==")"
        fields=m.split(row[1:-1]);assert len(fields)==2
        module=json.loads(m.joined(fields[0]))
        methods=fields[1];assert m.joined(methods[:2])=="&["and methods[-1][0]=="]"
        for entry in m.split(methods[2:-1]):
            assert entry[0][0]=="("and entry[-1][0]==")"
            pair=m.split(entry[1:-1]);assert len(pair)==2
            if len(pair[1])!=1 or not pair[1][0][0].isdigit():continue
            key=json.loads(m.joined(pair[0]))
            assert (module,key)not in rows
            rows[module,key]=pair[1][0]
    return rows


def arity_overrides(source):
    ts=list(m.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=="fn"and ts[i+1][0]=="bound_native_callable_value_arity")
    opening=next(i for i in range(start,len(ts))if ts[i][0]=="{")
    end=m.matching(ts,opening);rows={}
    for i in range(opening,end-9):
        if ts[i][0]!="(":continue
        close=m.matching(ts,i);pair=m.split(ts[i+1:close])
        if len(pair)!=2 or any(len(p)!=1 or not p[0][0].startswith('"')for p in pair):continue
        if m.joined(ts[close+1:close+4])!="=>Some(":continue
        if not ts[close+4][0].isdigit()or ts[close+5][0]!=")":continue
        rows[tuple(json.loads(p[0][0])for p in pair)]=ts[close+4]
    return rows


def projection_name(key):
    return "realm_native_"+re.sub(r"[^a-zA-Z0-9]+","_","_".join(key)).strip("_")+"_metadata"


def migrate(root,baseline,output,apply=False):
    oracle={d["name"]:d for d in baseline["declarations"]}
    table_file="crates/perry-runtime/src/object/native_module/callable_export_arity_table.rs"
    overrides_file="crates/perry-runtime/src/object/native_module/callable_exports.rs"
    literals=collections.defaultdict(list)
    for file,parse in ((table_file,arity_rows),(overrides_file,arity_overrides)):
        for key,literal in parse((root/file).read_text()).items():literals[key].append((file,literal))
    companions={p:json.loads(p.read_text())for p in root.glob("crates/*/src/**/*.realm.json")}
    entries=[];placements=[];unplaced=[];replacements=collections.defaultdict(dict);macro_bindings={}
    file="crates/perry-runtime/src/closure/registry.rs";path=root/file;source=path.read_text()
    start=source.index("pub static BOUND_METHOD_INFO:")
    end=source.index(";",start)+1
    following=source.index("\n",end)+1
    assert source.startswith("crate::realm_decl! {",following)
    line_end=source.index("\n",following)
    offset=source.rindex("}",following,line_end)
    line=source[:offset].count("\n")+1
    for companion,doc in companions.items():
        for values in doc["groups"].values():
            for d in values:
                if not d.get("function_info",{}).get("native_method")or d["length"]==0:continue
                candidate=copy.deepcopy(d);candidate["length"]=0
                for prop in candidate["properties"]:
                    if prop["key"]=="length":prop["value"]["value"]="0000000000000000"
                facts=m.native_default_facts(candidate)
                if facts is None:continue
                receiver=oracle[facts["receiver"]]
                key=(receiver["native_namespace"],d["captures"][1]["value"])
                sites=literals.get(key,[])
                if not sites or any(int(lit[0])!=d["length"]for _,lit in sites):
                    unplaced.append(d["name"]);continue
                macro=projection_name(key)
                assert macro_bindings.setdefault(macro,key)==key,"ambiguous native macro identifier"
                facts["spec"]="{length = "+str(d["length"])+"}"
                facts["emit"]="metadata("+macro+")"
                entries.append((d["name"],facts))
                placements.append(dict(name=d["name"],source=file,line=line,old_source="crates/"+oracle[d["name"]]["source"],
                    projection=macro,literal_sites=[dict(source=target,line=(root/target).read_text()[:lit[1]].count("\n")+1)for target,lit in sites]))
                for target,lit in sites:
                    previous=replacements[target].setdefault((lit[1],lit[2]),macro+"!(length)")
                    assert previous==macro+"!(length)"
    moved={p["name"]for p in placements}
    if apply:
        additions=("; ".join(m.render_logical(*entry)for entry in sorted(entries))+"; ") if entries else ""
        path.write_text(m.insert_at_sites(source,[(offset,additions)]))
        for file,edits in replacements.items():
            path=root/file;source=path.read_text();before=source.count("\n")
            for (start,end),replacement in sorted(edits.items(),reverse=True):source=source[:start]+replacement+source[end:]
            assert source.count("\n")==before
            path.write_text(source)
        for file,doc in companions.items():
            for key,values in doc["groups"].items():doc["groups"][key]=[d for d in values if d["name"]not in moved]
            assert any(doc["groups"].values()),"unexpected empty native companion"
            file.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+"\n")
    report=dict(placements=placements,unplaced=unplaced,entries_moved=len(placements),annotated_sites=1,
        cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements))
    output.mkdir(parents=True,exist_ok=True)
    (output/"migration-native-lengths.json").write_text(json.dumps(report,indent=2)+"\n")
    print({k:v for k,v in report.items()if k not in ("placements","unplaced")})
    print("unplaced",len(unplaced))

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("baseline",type=Path)
    p.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("--apply",action="store_true")
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
