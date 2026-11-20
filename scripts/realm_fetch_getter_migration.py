#!/usr/bin/env python3
"""Expose each generated Fetch getter's fn_info at its existing body declaration."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m

def expose(source):
    before=source.count('\n')
    old='($($body:ident => $name:literal),+ $(,)?)'
    new='($($body:ident => $name:literal, $info:expr),+ $(,)?)'
    assert source.count(old)==1
    source=source.replace(old,new,1)
    old='$($name => crate::fn_info!($body, 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),)+'
    assert source.count(old)==1
    source=source.replace(old,'$($name => $info,)+',1)
    start=source.index('fetch_getter_bodies! {')
    end=source.index('\n}',start)
    rows=source[start:end]
    rows,n=re.subn(r'(\w+) => ("[^"]+"),',lambda match:match[1]+' => '+match[2]+', crate::fn_info!('+match[1]+', 0; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR)),',rows)
    assert n>0
    result=source[:start]+rows+source[end:]
    assert result.count('\n')==before
    return result

def migrate(root,baseline,output,apply=False):
    path=root/'crates/perry-runtime/src/object/global_this/proto_methods.rs'
    source=path.read_text();updated=expose(source)
    original=m.simple_facts
    def facts(d):
        if not d.get('body','').startswith('crate::object::global_this::proto_methods::fetch_'):return None
        if d.get('header')!=dict(gc_type=4,reserved=16384,capture_flags=0)or d.get('captures')!=[]:return None
        candidate=dict(d,header=dict(gc_type=4,reserved=0,capture_flags=0))
        human=original(candidate)
        if human is not None:human['provider']='intrinsic_closure'
        return human
    # Discovery must see the concrete infos; restore on a report-only run.
    path.write_text(updated);m.simple_facts=facts
    try:return m.migrate(root,baseline,output,apply)
    finally:
        m.simple_facts=original
        if not apply:path.write_text(source)

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1])
    p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
