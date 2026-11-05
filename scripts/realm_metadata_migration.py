#!/usr/bin/env python3
"""One-time public-length projection migration; never a build input.

Only a literal in the enclosing birth call/row is replaced. Shared installers
with computed lengths are reported for a separate structured conversion.
"""
import argparse
import collections
import copy
import json
import re
from pathlib import Path
import realm_source_migration as migration


def human_length(declaration):
    info = declaration.get("function_info")
    if not info or info["native_method"]:
        return None
    empty_name=any(p.get("key")=="name" and p.get("value")==dict(kind="string",value="") for p in declaration.get("properties",[]))
    undefined_capture=declaration.get("captures")==[dict(kind="raw_word",value=hex(migration.CONSTANTS.get("TAG_UNDEFINED",0)))]
    default = migration.bare_length(info) if empty_name or undefined_capture else migration.default_length(info)
    if declaration["length"] == default:
        return None
    candidate = copy.deepcopy(declaration)
    candidate["length"] = default
    for prop in candidate["properties"]:
        if prop["key"] == "length":
            prop["value"]["value"] = migration.struct.pack(">d", default).hex()
    facts = migration.simple_facts(candidate)
    return None if facts is None else (facts, declaration["length"])


def birth_literal(source, site, length):
    ts = list(migration.tokens(source))
    opening = next(i for i,t in enumerate(ts) if t[2] == site["offset"])
    stack = []
    for i,t in enumerate(ts[:opening]):
        if t[0] in ("(", "[", "{"): stack.append(i)
        elif t[0] in (")", "]", "}"): stack.pop()
    if not stack or ts[stack[-1]][0] != "(": return None
    parent = stack[-1]
    end = migration.matching(ts, parent)
    args = migration.split(ts[parent+1:end])
    at = next((i for i,arg in enumerate(args) if arg and arg[0][1] <= site["offset"] <= arg[-1][2]), None)
    if at is None: return None
    matches = [arg[0] for arg in args[at+1:] if len(arg)==1 and arg[0][0] == str(length)]
    return matches[0] if len(matches)==1 else None


def temporal_rows(source):
    """Final Temporal births name the owner and list each method's public length."""
    ts = list(migration.tokens(source))
    result = {}
    for i,t in enumerate(ts):
        if t[0]!="populate_prototype" or i+1>=len(ts) or ts[i+1][0]!="(":
            continue
        if i and ts[i-1][0]=="fn": continue
        end=migration.matching(ts,i+1)
        args=migration.split(ts[i+2:end])
        if len(args)!=4 or len(args[1])!=1 or not args[1][0][0].startswith('"'):
            continue
        owner=json.loads(args[1][0][0])
        rows=args[3]
        if migration.joined(rows[:2])!="&[" or rows[-1][0]!="]": continue
        for row in migration.split(rows[2:-1]):
            if row[0][0]!="(" or row[-1][0]!=")": continue
            fields=migration.split(row[1:-1])
            if len(fields)!=2 or any(len(field)!=1 for field in fields): continue
            name,length=fields[0][0],fields[1][0]
            if not name[0].startswith('"') or not length[0].isdigit(): continue
            key=owner+".prototype."+json.loads(name[0])
            if key in result: raise ValueError(("duplicate Temporal birth",key))
            result[key]=length
    return result



def constructor_lengths(source):
    ts=list(migration.tokens(source))
    start=next(i for i,t in enumerate(ts[:-1])if t[0]=="fn"and ts[i+1][0]=="builtin_constructor_spec_length")
    opening=next(i for i in range(start,len(ts))if ts[i][0]=="{")
    end=migration.matching(ts,opening)
    at=next(i for i in range(opening,end)if ts[i][0]=="match")
    opening=next(i for i in range(at,end)if ts[i][0]=="{")
    closing=migration.matching(ts,opening)
    result={}
    for arm in migration.split(ts[opening+1:closing]):
        halves=migration.split(arm,"=>")
        if len(halves)!=2 or len(halves[1])!=1 or not halves[1][0][0].isdigit():continue
        for pattern in migration.split(halves[0],"|"):
            if len(pattern)!=1 or not pattern[0][0].startswith('"'):continue
            result[json.loads(pattern[0][0])]=halves[1][0]
    return result



def member_lengths(source,table,owner):
    ts=list(migration.tokens(source))
    starts=[i for i in range(len(ts)-1)if ts[i][0]=="const"and ts[i+1][0]==table]
    if len(starts)!=1:raise ValueError("member table must have one declaration")
    equals=next(i for i in range(starts[0],len(ts))if ts[i][0]=="=")
    if migration.joined(ts[equals+1:equals+3])!="&[":raise ValueError("member table must be a slice")
    end=migration.matching(ts,equals+2)
    result={}
    for row in migration.split(ts[equals+3:end]):
        if row[0][0]!="(" or row[-1][0]!=")":continue
        fields=migration.split(row[1:-1])
        if len(fields)!=2 or any(len(f)!=1 for f in fields):continue
        key,length=fields[0][0],fields[1][0]
        if not key[0].startswith('"') or not length[0].isdigit():continue
        name=owner+"."+json.loads(key[0])
        if name in result:raise ValueError("duplicate member name")
        result[name]=length
    return result



def http_lengths(source):
    ts=list(migration.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=="fn"and ts[i+1][0]=="build_http_prototype")
    opening=next(i for i in range(start,len(ts))if ts[i][0]=="{")
    end=migration.matching(ts,opening)
    result={}
    for i in range(opening,end-4):
        if ts[i][0]!="HttpProto" or ts[i+1][0]!="::" or ts[i+3][0]!="=>" or ts[i+4][0]!="{":continue
        owner="node:http."+ts[i+2][0]+".prototype."
        stop=migration.matching(ts,i+4)
        for j in range(i+5,stop):
            if ts[j][0]!="(":continue
            close=migration.matching(ts,j)
            fields=migration.split(ts[j+1:close])
            if len(fields)!=2 or any(len(f)!=1 for f in fields):continue
            key,length=fields[0][0],fields[1][0]
            if not key[0].startswith('"') or not length[0].isdigit():continue
            # A direct method call or a method row in the arm's literal list.
            if ts[j-1][0]not in ("method","[",","):continue
            name=owner+json.loads(key[0])
            if name in result:raise ValueError("duplicate HTTP method length")
            result[name]=length
    return result



def human_name(declaration):
    info=declaration.get("function_info")
    if not info or info["native_method"]:return None
    candidate=copy.deepcopy(declaration)
    default=migration.visible_name(declaration["name"])
    label=None
    for prop in candidate.get("properties",[]):
        if prop["key"]=="name"and prop["value"]["kind"]=="string":
            label=prop["value"]["value"]
            prop["value"]["value"]=default
    if label is None or label==default:return None
    facts=migration.simple_facts(candidate)
    return None if facts is None else (facts,label)


def http_accessor_labels(source):
    ts=list(migration.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=="fn"and ts[i+1][0]=="build_http_prototype")
    opening=next(i for i in range(start,len(ts))if ts[i][0]=="{")
    end=migration.matching(ts,opening)
    result={}
    for i in range(opening,end-4):
        if ts[i][0]!="HttpProto"or ts[i+1][0]!="::"or ts[i+3][0]!="=>"or ts[i+4][0]!="{":continue
        owner="node:http."+ts[i+2][0]+".prototype."
        stop=migration.matching(ts,i+4)
        def add(key,setter):
            name=owner+json.loads(key[0])
            for role in ("get","set")if setter else ("get",):
                path=name+".<"+role+">"
                if path in result:raise ValueError("duplicate HTTP accessor")
                result[path]=key
        for j in range(i+5,stop-3):
            if ts[j][0]=="accessor"and ts[j+1][0]=="(":
                close=migration.matching(ts,j+1);args=migration.split(ts[j+2:close])
                if len(args)==2 and all(len(a)==1 for a in args)and args[0][0][0].startswith('"')and args[1][0][0]in ("true","false"):
                    add(args[0][0],args[1][0][0]=="true")
            if migration.joined(ts[j:j+4])=="fornamein[":
                close=migration.matching(ts,j+3)
                if ts[close+1][0]!="{":continue
                last=migration.matching(ts,close+1)
                body=migration.joined(ts[close+2:last])
                if body not in ("accessor(name,true);","accessor(name,false);"):continue
                for row in migration.split(ts[j+4:close]):
                    if len(row)==1 and row[0][0].startswith('"'):add(row[0],body=="accessor(name,true);")
    return result


def migrate(root, baseline, output, apply=False, temporal=False, constructors=False, members=None, http=False, names=False):
    migration.CONSTANTS = migration.abi_constants(root)
    migration.FAMILIES = migration.constructor_families(root)
    sites = migration.scan(root)
    oracle = {d["name"]:d for d in baseline["declarations"]}
    temporal_file = "crates/perry-runtime/src/object/global_this/math_temporal.rs"
    constructor_file = "crates/perry-runtime/src/object/global_this_tables.rs"
    table = temporal_rows((root/temporal_file).read_text()) if temporal else constructor_lengths((root/constructor_file).read_text()) if constructors else {}
    if http or names:
        http_file="crates/perry-runtime/src/object/node_http_prototypes.rs"
        table=(http_accessor_labels if names else http_lengths)((root/http_file).read_text())
    if members:
        member_file,member_table,member_owner=members
        table=member_lengths((root/member_file).read_text(),member_table,member_owner)
    groups = collections.defaultdict(list)
    companions = {}
    unplaced = []
    for file in sorted(root.glob("crates/*/src/**/*.realm.json")):
        doc = json.loads(file.read_text())
        companions[file] = doc
        for entries in doc["groups"].values():
            for entry in entries:
                result = (human_name if names else human_length)(entry)
                if result is None: continue
                facts, length = result
                candidates = [s for s in sites[entry["body"].split("::")[-1]]
                              if all(entry["function_info"].get(k)==v for k,v in s["signature"].items())]
                candidates.sort(key=lambda s:migration.placement_rank(s,entry["body"],"crates/"+oracle[entry["name"]]["source"]))
                for site in candidates:
                    if names:
                        literal=table.get(entry["name"])
                        if literal is not None and json.loads(literal[0])!=length:literal=None
                        target=http_file
                    elif http:
                        literal=table.get(entry["name"])
                        if literal is not None and literal[0]!=str(length):literal=None
                        target=http_file
                    elif members:
                        literal=table.get(entry["name"])
                        if literal is not None and literal[0]!=str(length):literal=None
                        target=member_file
                    elif constructors:
                        literal = table.get(entry["name"]) if entry["kind"]=="constructor" else None
                        if literal is not None and literal[0]!=str(length):literal=None
                        target=constructor_file
                    elif temporal:
                        literal = table.get(entry["name"]) if entry["body"].endswith("::temporal_proto_method_thunk") else None
                        if literal is not None and literal[0]!=str(length): literal=None
                        target=temporal_file
                    else:
                        literal = birth_literal((root/site["path"]).read_text(), site, length)
                        target=site["path"]
                    if literal is not None:
                        groups[(site["path"],site["offset"])].append((entry, facts, site, literal, target))
                        break
                else: unplaced.append(entry["name"])
    edits = collections.defaultdict(list)
    placements = []
    projections = {}
    for entries in groups.values():
        for entry,_,_,literal,target in sorted(entries,key=lambda row:row[0]["name"]):
            key=(target,literal)
            projections.setdefault(key,"realm_"+re.sub(r"[^a-z0-9]+","_",entry["name"].lower()).strip("_")+("_name_metadata"if names else "_metadata"))
    for (file, offset), entries in groups.items():
        entries.sort(key=lambda row:row[0]["name"])
        site = entries[0][2]
        annotation = site["annotation"]
        rendered = list(annotation["facts"]) if annotation else []
        for entry,facts,_,literal,target in entries:
            macro=projections[(target,literal)]
            spec="name = property_key" if names else f"length = {entry['length']}"
            rendered.append(migration.render(entry["name"],facts) +
                            f", spec = {{{spec}}}, emit = metadata({macro})")
            previous = oracle[entry["name"]]
            placements.append(dict(name=entry["name"],source=file,line=site["line"],
                                   old_source="crates/"+previous["source"],old_line=previous["line"],
                                   projection=macro,literal_source=target,literal_line=(root/target).read_text()[:literal[1]].count("\n")+1))
        inner = rendered[0] if len(rendered)==1 else "realm_decl! { "+"; ".join(rendered)+"; }"
        start,end = (annotation["start"],annotation["end"]) if annotation else (offset,offset)
        edits[file].append((start,end,"#[realm("+inner+")]" + ("" if annotation else " ")))
    for (target,literal),macro in projections.items():
        prefix = "" if target.startswith("crates/perry-runtime/") else "perry_runtime::"
        edits[target].append((literal[1],literal[2],prefix+macro+("!(name)"if names else "!(length)")))
    moved = {p["name"] for p in placements}
    deleted = []
    if apply:
        for file,replacements in edits.items():
            path=root/file
            source=path.read_text()
            before=source.count("\n")
            for start,end,value in sorted(replacements,reverse=True):
                source=source[:start]+value+source[end:]
            assert source.count("\n")==before
            path.write_text(source)
        for file,doc in companions.items():
            for group,entries in doc["groups"].items():
                doc["groups"][group]=[entry for entry in entries if entry["name"]not in moved]
            if any(doc["groups"].values()):
                file.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+"\n")
            else:
                deleted.append(str(file.relative_to(root)))
                file.unlink()
                path=file.with_name(file.name.replace(".realm.json",".rs"))
                source=path.read_text()
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(file.name)+r'#\*"\);[^\n]*$',re.M)
                source,count=pattern.subn("",source)
                assert count==1,(file,count)
                path.write_text(source.rstrip()+"\n")
    report=dict(placements=placements,unplaced=unplaced,deleted_companions=deleted,
                entries_moved=len(placements),annotated_sites=len({(p["source"],p["line"])for p in placements}),files_changed=len(edits),
                cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements))
    output.mkdir(parents=True,exist_ok=True)
    (output/"migration-public-length.json").write_text(json.dumps(report,indent=2)+"\n")
    print({k:v for k,v in report.items()if k not in ("placements","unplaced","deleted_companions")})
    print("unplaced",len(unplaced),"deleted companions",len(deleted))


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline",type=Path)
    parser.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--apply",action="store_true")
    parser.add_argument("--temporal",action="store_true")
    parser.add_argument("--constructors",action="store_true")
    parser.add_argument("--members",nargs=3,metavar=("FILE","TABLE","OWNER"))
    parser.add_argument("--http",action="store_true")
    parser.add_argument("--http-names",action="store_true")
    args=parser.parse_args()
    migrate(args.root,json.loads(args.baseline.read_text()),args.output,args.apply,args.temporal,args.constructors,args.members,args.http,args.http_names)
