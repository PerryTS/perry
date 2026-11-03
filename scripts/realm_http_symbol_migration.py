#!/usr/bin/env python3
"""Bind symbol-keyed HTTP method captures to their projected public names."""
import argparse
import copy
import json
import struct
from pathlib import Path
import realm_source_migration as m
NAMES={
 'node:http.IncomingMessage.prototype["@runtime:kAttachAbortSignal"]':'realm_http_symbol_attach_public',
 'node:http.IncomingMessage.prototype["@runtime:kDetachAbortSignal"]':'realm_http_symbol_detach_public',
 'node:http.OutgoingMessage.prototype["@runtime:nodejs.rejection"]':'realm_http_symbol_rejection_public',
}
def classify(d,original):
    if d['name']not in NAMES:return None
    label=next(p['value']['value']for p in d['properties']if p['key']=='name')
    assert d['captures']==[dict(kind='bytes',value=label),dict(kind='raw_word',value=hex(len(label.encode())))]
    c=copy.deepcopy(d);name=m.visible_name(d['name']);length=m.default_length(d['function_info'])
    c['captures']=[dict(kind='bytes',value=name),dict(kind='raw_word',value=hex(len(name.encode())))]
    c['length']=length
    for p in c['properties']:
        if p['key']=='length':p['value']['value']=struct.pack('>d',float(length)).hex()
        if p['key']=='name':p['value']['value']=name
    human=original(c);assert human is not None,d['name']
    human.update(capture='public_name',spec='{name = '+json.dumps(label)+', length = 0}',emit='metadata('+NAMES[d['name']]+')')
    return human
def migrate(root,baseline,output,apply=False):
    original=m.simple_facts;m.simple_facts=lambda d:classify(d,original)
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==3
    if apply:
        path=root/'crates/perry-runtime/src/object/node_http_prototypes.rs';s=path.read_text()
        replacements=[
          ("name: &'static str| {\n        let value = named_closure(&scope, method_info(name), name, 0);",
           "name: &'static str, length: u32| {\n        let value = named_closure(&scope, method_info(name), name, length);"),
          ('symbol(&raw const layouts::NP_SYMBOL_NODEJS_REJECTION, "destroy");',
           'symbol(&raw const layouts::NP_SYMBOL_NODEJS_REJECTION, realm_http_symbol_rejection_public!(name), realm_http_symbol_rejection_public!(length));'),
        ]
        for short,kind in [('ATTACH','attach'),('DETACH','detach')]:
            label='k'+short.title()+'AbortSignal'
            old='&raw const layouts::NP_SYMBOL_K_'+short+'_ABORT_SIGNAL,\n                '+json.dumps(label)+','
            new='&raw const layouts::NP_SYMBOL_K_'+short+'_ABORT_SIGNAL,\n                realm_http_symbol_'+kind+'_public!(name), realm_http_symbol_'+kind+'_public!(length),'
            replacements.append((old,new))
        for old,new in replacements:
            assert s.count(old)==1,(old,s.count(old))
            assert old.count('\n')==new.count('\n')
            s=s.replace(old,new,1)
        path.write_text(s)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
