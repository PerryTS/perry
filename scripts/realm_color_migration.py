#!/usr/bin/env python3
"""Migration record for the AnsiStyle map and its numeric array births."""
import argparse
import json
import re
import struct
from pathlib import Path
import realm_source_migration as m

def color_rows(source):
    ts=list(m.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=="const"and ts[i+1][0]=="INSPECT_COLOR_STYLES")
    equals=next(i for i in range(start,len(ts))if ts[i][0]=="=")
    assert m.joined(ts[equals+1:equals+3])=="&["
    end=m.matching(ts,equals+2)
    rows=[]
    for row in m.split(ts[equals+3:end]):
        assert m.joined(row[:2])=="AnsiStyle{" and row[-1][0]=="}"
        fields={part[0][0]:m.joined(part[2:])for part in m.split(row[2:-1])}
        assert set(fields)=={"name","open","close"}
        rows.append((json.loads(fields["name"]),int(fields["open"]),int(fields["close"])))
    assert len({r[0]for r in rows})==len(rows)
    return rows

def birth_site(source):
    ts=list(m.tokens(source))
    start=next(i for i in range(len(ts)-1)if ts[i][0]=="fn"and ts[i+1][0]=="util_inspect_colors")
    opening=next(i for i in range(start,len(ts))if ts[i][0]=="{")
    end=m.matching(ts,opening)
    alloc=next(i for i in range(opening,end)if ts[i][0]=="js_object_alloc")
    line_start=source.rfind("\n",0,ts[alloc][1])+1
    previous=source.rfind("\n",0,line_start-1)+1
    assert not source[previous:line_start].strip()
    return previous,source[:previous].count("\n")+1

def migrate(root,baseline,output,apply=False):
    owner=root/"crates/perry-runtime/src/object/native_module/callable_exports.rs"
    source=owner.read_text();offset,line=birth_site(source)
    rows=color_rows((root/"crates/perry-runtime/src/util_style_text.rs").read_text())
    roots=[d for d in baseline["declarations"]if d["name"].endswith(".inspect.colors")]
    assert len(roots)==1
    parent=roots[0];name=parent["name"]
    assert parent["own_key_order"]==[r[0]for r in rows]
    selected={name}
    oracle={d["name"]:d for d in baseline["declarations"]}
    for key,open_,close in rows:
        child=name+"."+key
        d=oracle[child]
        assert d["own_key_order"]==["0","1","length"]
        assert [p["value"]for p in d["properties"]]==[dict(kind="number_bits",value=struct.pack(">d",v).hex())for v in (open_,close,2)]
        selected.add(child)
    placements=[dict(name=n,source=str(owner.relative_to(root)),line=line,
                     old_source="crates/"+oracle[n]["source"],old_line=oracle[n]["line"])for n in sorted(selected)]
    if apply:
        before=source.count("\n")
        source=source[:offset]+'        crate::realm_decl! { '+json.dumps(name)+', provider = ansi_colors; }'+source[offset:]
        assert source.count("\n")==before
        owner.write_text(source)
        removed=0
        for file in root.glob("crates/*/src/**/*.realm.json"):
            doc=json.loads(file.read_text())
            for key,entries in doc["groups"].items():
                doc["groups"][key]=[d for d in entries if d["name"]not in selected]
                removed+=len(entries)-len(doc["groups"][key])
            assert any(doc["groups"].values()),"unexpected empty companion"
            file.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+"\n")
        assert removed==len(selected)
    report=dict(entries_moved=len(placements),annotated_sites=1,
                cross_file_entries=sum(p["source"]!=p["old_source"]for p in placements),
                placements=placements)
    output.mkdir(parents=True,exist_ok=True)
    (output/"migration-colors.json").write_text(json.dumps(report,indent=2)+"\n")
    print({k:v for k,v in report.items()if k!="placements"})

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("baseline",type=Path)
    p.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("--apply",action="store_true")
    a=p.parse_args()
    migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
