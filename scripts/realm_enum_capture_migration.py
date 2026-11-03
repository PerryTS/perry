#!/usr/bin/env python3
"""One-time enum property capture migration; consumes existing typed source rows."""
import argparse
import copy
import json
import re
import struct
from pathlib import Path
import realm_source_migration as m


def property_bindings(source):
    ts=list(m.tokens(source))
    bindings={}
    for i,t in enumerate(ts[:-2]):
        if t[0]!="enum" or ts[i+2][0]!="{":continue
        if i<7 or m.joined(ts[i-7:i])!="#[repr(u8)]":continue
        name=ts[i+1][0]
        end=m.matching(ts,i+2)
        rows=m.split(ts[i+3:end])
        if any(len(row)!=1 for row in rows):continue
        variants={row[0][0]:index for index,row in enumerate(rows)}
        keys={}
        for j in range(len(ts)-6):
            row=ts[j:j+7]
            if row[0][0]!="(" or not row[1][0].startswith('"') or row[2][0]!="," or row[3][0]!=name or row[4][0]!="::" or row[6][0]not in (",",")"):continue
            key=json.loads(row[1][0]);variant=row[5][0]
            if variant not in variants:raise ValueError("unknown enum variant")
            keys.setdefault(key,set()).add(variants[variant])
        for key,indices in keys.items():
            if len(indices)==1:
                bits=struct.unpack(">Q",struct.pack(">d",indices.pop()))[0]
                bindings.setdefault(key,[]).append((name,hex(bits)))
    return bindings


def migrate(root,baseline,output,apply=False):
    m.CONSTANTS=m.abi_constants(root)
    sites=m.scan(root)
    source_bindings={}
    for candidates in sites.values():
        for site in candidates:
            path=site["path"]
            if path not in source_bindings:
                source_bindings[path]=property_bindings((root/path).read_text())
    original=m.simple_facts
    def specialized(d):
        direct=original(d)
        if direct is not None:return None
        captures=d.get("captures")
        if not captures or len(captures)!=1 or captures[0]["kind"]!="raw_word":return None
        key=m.visible_name(d["name"]).removeprefix("get ").removeprefix("set ")
        choices=set()
        for site in sites[d.get("body","").split("::")[-1]]:
            for enum,bits in source_bindings[site["path"]].get(key,[]):
                if captures[0]["value"]==bits:choices.add(enum)
        if len(choices)!=1:return None
        candidate=copy.deepcopy(d)
        candidate["captures"]=[dict(kind="raw_word",value=hex(m.CONSTANTS["TAG_UNDEFINED"]))]
        facts=original(candidate)
        if facts is None:return None
        facts.pop("receiver")
        facts["capture"]="enum_property("+choices.pop()+")"
        return facts
    m.simple_facts=specialized
    try:m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline",type=Path)
    parser.add_argument("--root",type=Path,default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--apply",action="store_true")
    args=parser.parse_args()
    migrate(args.root,json.loads(args.baseline.read_text()),args.output,args.apply)
