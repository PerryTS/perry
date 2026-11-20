#!/usr/bin/env python3
"""Move shared static bodies, keeping numeric selectors in their existing birth calls."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m

NAMES=['node:net.BlockList.isBlockList','node:net.SocketAddress.isSocketAddress','node:net.SocketAddress.parse','node:crypto.ECDH.convertKey']

def classify(d,original):
    if d['name'] not in NAMES:return None
    candidate=copy.deepcopy(d);candidate['captures']=[]
    length=m.default_length(d['function_info'])
    candidate['length']=length
    import struct
    for p in candidate['properties']:
        if p['key']=='length':p['value']['value']=struct.pack('>d',float(length)).hex()
    human=original(candidate)
    assert human is not None,d['name']
    human['capture']='[number_argument(own_static, kind)]'
    if d['length']!=length:
        human.update(spec='{length = '+str(d['length'])+'}',emit='metadata(realm_native_predicate_public)')
    return human

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    def facts(d):return classify(d,original)
    m.simple_facts=facts
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==4
    if apply:
        path=root/'crates/perry-runtime/src/object/native_module/constructor_shapes.rs'
        source=path.read_text()
        for name,kind in [('isBlockList',0),('isSocketAddress',1),('parse',2)]:
            old=f'own_static(ctor.get_nanbox_f64(), "{name}", {kind}, 1);'
            new=f'own_static(ctor.get_nanbox_f64(), "{name}", {kind}, realm_native_predicate_public!(length));'
            assert source.count(old)==1
            source=source.replace(old,new,1)
        path.write_text(source)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
