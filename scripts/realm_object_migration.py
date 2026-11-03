#!/usr/bin/env python3
"""Migration record: scalar native object births become executable declarations."""
import argparse
import collections
import json
import re
import struct
from pathlib import Path
import realm_source_migration as m

def scalar(ts):
    text=m.joined(ts)
    if len(ts)==4 and ts[1][0]=="(" and ts[3][0]==")":
        if ts[0][0]=="native_string_value" and ts[2][0].startswith('"'):
            value=json.loads(ts[2][0])
            return dict(kind="string",value=value),"string("+ts[2][0]+")"
        if ts[0][0]=="native_bool_value"and ts[2][0]in ("true","false"):
            return dict(kind="boolean",value=ts[2][0]=="true"),"boolean("+ts[2][0]+")"
    if re.fullmatch(r"-?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?",text):
        return dict(kind="number_bits",value=struct.pack(">d",float(text)).hex()),"number("+text+")"
    return None

def births(source):
    ts=list(m.tokens(source));excluded=m.test_ranges(ts);results=[]
    for i,t in enumerate(ts):
        if t[0]!="let"or i+2>=len(ts)or not re.fullmatch(r"\w+",ts[i+1][0]):continue
        if any(start<=t[1]<end for start,end in excluded):continue
        statement=m.split(ts[i:],";")[0]
        if m.joined(statement[2:])!="=js_object_alloc(0,0)":continue
        variable=statement[1][0];last=i+len(statement)
        first_end=ts[last][2];at=last+1;fields=[];invalid=False
        while at<len(ts)and ts[at][0]=="native_set_field":
            if ts[at+1][0]!="(":invalid=True;break
            close=m.matching(ts,at+1);args=m.split(ts[at+2:close])
            if len(args)!=3 or m.joined(args[0])!=variable or len(args[1])!=1 or not args[1][0][0].startswith('"')or ts[close+1][0]!=";":
                invalid=True;break
            value=scalar(args[2])
            if value is None:invalid=True;break
            key=json.loads(args[1][0][0])
            fields.append(dict(key=key,value=value[0],render=json.dumps(key,ensure_ascii=False)+" => "+value[1]+";",start=ts[at][1],end=ts[close+1][2]))
            at=close+2
        if invalid or not fields or len({f["key"]for f in fields})!=len(fields):continue
        if any("\n"in source[f["start"]:f["end"]]for f in fields):continue
        keys=[f["key"]for f in fields]
        def index(key):
            return int(key)if re.fullmatch(r"(?:0|[1-9]\d*)",key)and int(key)<2**32-1 else None
        keys.sort(key=lambda k:(0,index(k))if index(k)is not None else (1,0))
        facts=dict(kind="namespace",properties=sorted([dict(key=f["key"],symbol=False,attributes=7,value=f["value"])for f in fields],key=lambda p:p["key"]),
                   own_key_order=keys,own_symbol_order=[],header=dict(gc_type=2,reserved=0,class_id=0),
                   prototype="Object.prototype",births=["Object.prototype"])
        results.append(dict(offset=t[1],first_end=first_end,line=source[:t[1]].count("\n")+1,variable=variable,fields=fields,facts=facts))
    return results

def migrate(root,baseline,output,apply=False):
    oracle={d["name"]:d for d in baseline["declarations"]}
    companions={p:json.loads(p.read_text())for p in root.glob("crates/*/src/**/*.realm.json")}
    fingerprints=collections.defaultdict(list)
    for doc in companions.values():
        for entries in doc["groups"].values():
            for d in entries:
                facts={k:v for k,v in d.items()if k!="name"}
                fingerprints[json.dumps(facts,sort_keys=True)].append(d["name"])
    placements=[];edits=collections.defaultdict(list);selected=set();ambiguous=[]
    for path in root.glob("crates/perry-runtime/src/**/*.rs"):
        if "tests"in path.parts or path.stem.endswith("_tests"):continue
        source=path.read_text()
        if "fn native_set_field("not in source:continue
        for birth in births(source):
            names=fingerprints.get(json.dumps(birth["facts"],sort_keys=True),[])
            if len(names)!=1:
                if names:ambiguous.append(names)
                continue
            name=names[0]
            if name in selected:continue
            selected.add(name)
            edits[path].append((birth["offset"],birth["first_end"],"crate::realm_decl! { object "+json.dumps(name)+", "+birth["variable"]+";"))
            for i,field in enumerate(birth["fields"]):
                edits[path].append((field["start"],field["end"],field["render"]+(" }"if i==len(birth["fields"])-1 else "")))
            placements.append(dict(name=name,source=str(path.relative_to(root)),line=birth["line"],old_source="crates/"+oracle[name]["source"],old_line=oracle[name]["line"]))
    if apply:
        for path,replacements in edits.items():
            source=path.read_text();before=source.count("\n")
            for start,end,text in sorted(replacements,reverse=True):source=source[:start]+text+source[end:]
            assert source.count("\n")==before
            path.write_text(source)
        for path,doc in companions.items():
            for key,entries in doc["groups"].items():doc["groups"][key]=[d for d in entries if d["name"]not in selected]
            assert any(doc["groups"].values()),"unexpected empty object companion"
            path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+"\n")
    report=dict(entries_moved=len(placements),annotated_sites=len(placements),
        cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements),
        placements=placements,ambiguous=ambiguous)
    output.mkdir(parents=True,exist_ok=True);(output/"migration-objects.json").write_text(json.dumps(report,indent=2)+"\n")
    print({k:v for k,v in report.items()if k not in ("placements","ambiguous")})

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("baseline",type=Path)
    p.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    p.add_argument("--output",type=Path,required=True);p.add_argument("--apply",action="store_true")
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
