#!/usr/bin/env python3
"""Derive stream static captures from the actual operation constants and rows."""
import argparse
import copy
import json
import re
import struct
from pathlib import Path
import realm_source_migration as m

def bindings(source):
    values={name:struct.pack('>d',float(value)).hex()for name,value in re.findall(r'const (STREAM_STATIC_\w+): f64 = ([0-9.]+);',source)}
    ts=list(m.tokens(source));result={}
    for i in range(len(ts)-2):
        if ts[i][0]not in ('"Readable"','"Writable"','"Duplex"')or m.joined(ts[i+1:i+3])!='=>{':continue
        end=m.matching(ts,i+2);owner='node:stream.'+json.loads(ts[i][0])
        for j in range(i+3,end-1):
            if ts[j][0]!='attach_stream_static'or ts[j+1][0]!='(':continue
            close=m.matching(ts,j+1);args=m.split(ts[j+2:close])
            if len(args)!=4 or len(args[1])!=1 or len(args[2])!=1 or len(args[3])!=1:continue
            constant=args[2][0][0]
            if constant not in values or not args[3][0][0].isdigit():continue
            result[owner+'.'+json.loads(args[1][0][0])]=(constant,values[constant],args[3][0])
    return result

def migrate(root,baseline,output,apply=False):
    path=root/'crates/perry-runtime/src/object/native_module_stream.rs'
    rows=bindings(path.read_text());original=m.simple_facts;lengths=set()
    def facts(d):
        if not d.get('body','').endswith('::stream_static_method_thunk')or d['name']not in rows:return None
        constant,bits,length=rows[d['name']]
        if d['captures']!=[dict(kind='raw_word',value=hex(int(bits,16)))]:return None
        candidate=copy.deepcopy(d);candidate['captures']=[]
        abi=m.default_length(d['function_info'])
        candidate['length']=abi
        for prop in candidate['properties']:
            if prop['key']=='length':prop['value']['value']=struct.pack('>d',abi).hex()
        human=original(candidate)
        if human is None:return None
        human['capture']='number('+constant+')'
        if d['length']!=abi:
            assert d['length']==int(length[0])==1
            human.update(spec='{length = 1}',emit='metadata(realm_stream_static_one_argument)')
            lengths.add(d['name'])
        return human
    m.simple_facts=facts
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    if apply:
        source=path.read_text();rows=bindings(source)
        for name in sorted(lengths,key=lambda n:rows[n][2][1],reverse=True):
            literal=rows[name][2]
            source=source[:literal[1]]+'realm_stream_static_one_argument!(length)'+source[literal[2]:]
        path.write_text(source)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
