#!/usr/bin/env python3
"""Migration record for physical cells sharing an existing named body info."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m


def named_infos(source):
    ts=list(m.tokens(source))
    sites=[]
    for i,t in enumerate(ts):
        if t[0] not in ("static","const") or i+2>=len(ts):continue
        start=source.rfind("\n",0,t[1])+1
        prefix=source[start:t[1]]
        visibility=re.fullmatch(r"\s*(?:pub(?:\([^()]*\))?\s+)?",prefix)
        if not visibility or (i and ts[i-1][0]=="]"):continue
        equals=next((j for j in range(i+2,len(ts))if ts[j][0]in ("=",";")),None)
        if equals is None or ts[equals][0]!="=" or "JsFunctionInfo"not in [x[0]for x in ts[i+2:equals]]:continue
        expression=m.split(ts[equals+1:],";")[0]
        opening=next((j for j,x in enumerate(expression)if x[0]=="("),None)
        if opening is None:continue
        close=m.matching(expression,opening)
        constructor=m.joined(expression[:opening])
        args=m.split(expression[opening+1:close])
        if constructor=="ns_info!":
            signature=m.joined(expression[opening+1:close])
        elif constructor.endswith("JsFunctionInfo::of_native_args") and len(args)==2:
            signature="native_args "+m.joined(args[0])+","+m.joined(args[1])
        elif constructor.endswith("JsFunctionInfo::of") and len(args)==1:
            cast=next((j for j,x in enumerate(args[0])if x[0]=="as"),None)
            if cast is None:continue
            arity=next((x[0][6:]for x in args[0][cast+1:]if x[0].startswith("JsBody") and x[0][6:].isdigit()),None)
            if arity is None:continue
            signature=m.joined(args[0][:cast])+","+arity
        else:continue
        at=close+1;builders=[]
        while at<len(expression):
            if expression[at][0]!="." or at+2>=len(expression) or expression[at+2][0]!="(":break
            end=m.matching(expression,at+2)
            builders.append(m.joined(expression[at+1:end+1]))
            at=end+1
        if at!=len(expression):continue
        if builders:signature+=";"+",".join(builders)
        try:body,abi=m.signature(list(m.tokens(signature)))
        except (ValueError,KeyError,NameError,SyntaxError):continue
        offset=start+len(prefix)-len(prefix.lstrip())
        previous=start-1
        while previous>=0:
            begin=source.rfind("\n",0,previous)+1
            line=source[begin:previous]
            if not line.lstrip().startswith("///"):break
            offset=begin+len(line)-len(line.lstrip())
            previous=begin-1
        sites.append(dict(binding=ts[i+1][0],body=body,signature=abi,
                          offset=offset,declaration_end=ts[equals+1+len(expression)][2],line=source[:offset].count("\n")+1))
    return sites


def bare_receiver_facts(d):
    f=d.get("function_info")
    if not f or f["native_method"]:return None
    length=m.bare_length(f)
    keys=["length","name"]+([]if f["flags"]&512 else ["prototype"])
    expected=dict(name=d["name"],kind=d["kind"],length=length,
                  properties=[
                      dict(key="length",symbol=False,attributes=4,value=dict(kind="number_bits",value=m.struct.pack(">d",length).hex())),
                      dict(key="name",symbol=False,attributes=4,value=dict(kind="string",value="")),
                  ],own_key_order=keys,own_symbol_order=[],
                  header=dict(gc_type=4,reserved=m.CONSTANTS["GC_LAYOUT_POINTER_FREE"],capture_flags=0),
                  prototype="Function.prototype",body=d["body"],function_info=f,
                  captures=[dict(kind="raw_word",value=hex(m.CONSTANTS["TAG_UNDEFINED"]))],
                  info_symbol=d["info_symbol"],births=d["births"])
    if d!=expected:return None
    facts=dict(provider="closure",receiver="undefined")
    if d["kind"]!=m.default_kind(d["name"]):facts["kind"]=d["kind"]
    births=[b for b in d["births"]if b!="Function.prototype"]
    if births:facts["births"]=births
    return facts


def migrate(root,baseline,output,apply=False):
    m.CONSTANTS=m.abi_constants(root)
    m.FAMILIES=m.constructor_families(root)
    oracle={d["name"]:d for d in baseline["declarations"]}
    sites=collections.defaultdict(list)
    for path in root.glob("crates/*/src/**/*.rs"):
        if "tests"in path.parts or path.stem.endswith("_tests"):continue
        source=path.read_text()
        if "JsFunctionInfo"not in source:continue
        for site in named_infos(source):
            site["path"]=str(path.relative_to(root))
            sites[site["body"].split("::")[-1]].append(site)
    companions={p:json.loads(p.read_text())for p in root.glob("crates/*/src/**/*.realm.json")}
    groups=collections.defaultdict(list)
    placements=[];unplaced=[]
    for path,doc in companions.items():
        for entries in doc["groups"].values():
            for d in entries:
                facts=m.simple_facts(d)
                if facts is None:continue
                candidates=[s for s in sites[d["body"].split("::")[-1]]
                            if all(d["function_info"].get(k)==v for k,v in s["signature"].items())]
                if not candidates:unplaced.append(d["name"]);continue
                old=oracle[d["name"]]
                site=min(candidates,key=lambda s:m.placement_rank(s,d["body"],"crates/"+old["source"]))
                groups[(site["path"],site["offset"],site["declaration_end"])].append((d["name"],facts))
                placements.append(dict(name=d["name"],source=site["path"],line=site["line"],
                                       old_source="crates/"+old["source"],old_line=old["line"],
                                       info_binding=site["binding"]))
    edits=collections.defaultdict(list)
    for (file,offset,end),entries in groups.items():
        entries.sort()
        text=" realm { "+"; ".join(m.render(*entry)for entry in entries)+"; } }"
        edits[file].extend([(offset,"crate::realm_decl! { info "),(end,text)])
    moved={p["name"]for p in placements};deleted=[]
    if apply:
        for file,insertions in edits.items():
            path=root/file;path.write_text(m.insert_at_sites(path.read_text(),insertions))
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
                entries_moved=len(placements),annotated_sites=len(groups),
                cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements))
    output.mkdir(parents=True,exist_ok=True)
    (output/"migration-named-infos.json").write_text(json.dumps(report,indent=2)+"\n")
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
