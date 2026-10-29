#!/usr/bin/env python3
"""Move bare closure captures using their existing selector rows and realm references."""
import argparse
import copy
import json
import re
from pathlib import Path
import realm_source_migration as m

def selector(source,key,floating):
    values=set()
    for name,literal in re.findall(r'\("([^"]+)",\s*([0-9.]+)(?:_i64)?\s*\)',source):
        if name==key:
            import struct
            values.add(hex(struct.unpack('>Q',struct.pack('>d',float(literal)))[0]if floating else int(literal)))
    return next(iter(values))if len(values)==1 else None

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    def facts(d):
        body=d.get('body','').split('::')[-1]
        if body in ('lazy_state_get','lazy_state_set'):
            key=m.visible_name(d['name']).removeprefix('get ').removeprefix('set ')
            bits=selector((root/'crates/perry-runtime/src/node_stream/native_hooks.rs').read_text(),key,True)
            if d['captures']!=[dict(kind='raw_word',value=bits)]:return None
            binding='[number_property]'
        elif body=='ns_ee_async_resource_getter':
            key=m.visible_name(d['name']).removeprefix('get ')
            bits=selector((root/'crates/perry-runtime/src/node_stream_dispatch.rs').read_text(),key,False)
            if d['captures']!=[dict(kind='raw_word',value=hex(m.CONSTANTS['TAG_UNDEFINED'])),dict(kind='raw_word',value=bits)]:return None
            binding='[undefined, word_property]'
        elif d['name']=='process.stdin["@@asyncIterator"]':
            if d['captures']!=[dict(kind='reference',value='process.stdin')]:return None
            binding='[ref(process.stdin)]'
        else:return None
        candidate=copy.deepcopy(d)
        candidate['captures']=[dict(kind='raw_word',value=hex(m.CONSTANTS['TAG_UNDEFINED']))]
        candidate['header']['reserved']=m.CONSTANTS['GC_LAYOUT_POINTER_FREE']
        human=original(candidate)
        if human is None:return None
        human.pop('receiver')
        human['capture']=binding
        if binding.startswith('[ref('):human.pop('births',None)
        return human
    m.simple_facts=facts
    try:return m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
