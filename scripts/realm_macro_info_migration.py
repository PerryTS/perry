#!/usr/bin/env python3
"""Expose each domain method's fn_info! at its existing installer invocation."""
import argparse
import json
import re
from pathlib import Path
import realm_source_migration as m

def expose(source):
    old='''($name:literal, $body:ident, $n:tt) => {
            proto.method($name, perry_runtime::fn_info!(
                $body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN)
            ), $n)
        };'''
    new='''($name:literal, $info:expr, $n:tt) => {
            proto.method($name, $info,
                // The call site owns the function info and its realm facts.
                $n)
        };'''
    assert source.count(old)==1
    source=source.replace(old,new)
    source=source.replace('fn install_domain_prototype(', '#[rustfmt::skip] fn install_domain_prototype(',1)
    pattern=re.compile(r'method!\(("[^"]+"), (domain_\w+_thunk), (\d+)\);')
    def rewrite(match):
        name,body,arity=match.groups()
        return f'method!({name}, perry_runtime::fn_info!({body}, {arity}; with_declared({arity}), with_flags(perry_runtime::closure::FN_BUILTIN)), {arity});'
    result,count=pattern.subn(rewrite,source)
    assert count==7,count
    assert result.count('\n')==source.count('\n')
    return result

def migrate(root,baseline,output,apply=False):
    file=root/'crates/perry-stdlib/src/domain.rs'
    source=expose(file.read_text())
    if apply:file.write_text(source)
    else:
        output.mkdir(parents=True,exist_ok=True)
        (output/'proposed-domain.rs').write_text(source)
        print('prepared 7 concrete fn_info sites')
        return
    original=m.simple_facts
    m.simple_facts=lambda d:original(d) if d.get('body','').startswith('perry_stdlib::domain::') else None
    try:m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
