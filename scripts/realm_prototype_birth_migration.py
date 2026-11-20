#!/usr/bin/env python3
"""Migration record: human prototype members wrap their existing allocation."""
import argparse
import collections
import json
import math
import re
import struct
from pathlib import Path
import realm_source_migration as m

def key_text(key,symbol=False):
    if symbol:
        return '[Symbol.'+key[2:]+']' if key.startswith('@@') else '['+json.dumps(key,ensure_ascii=False)+']'
    return key if re.fullmatch(r'[A-Za-z_]\w*',key)else json.dumps(key,ensure_ascii=False)

def default_target(owner,key,symbol):
    if not symbol and key=='constructor' and owner.endswith('.prototype'):return owner.removesuffix('.prototype')
    return owner+('['+json.dumps(key,ensure_ascii=False)+']' if symbol else '.'+key)

def scalar(value):
    kind=value['kind']
    if kind=='string':return json.dumps(value['value'],ensure_ascii=False)
    if kind=='dynamic':
        binding=value['value']
        return 'target('+binding.removeprefix('target.')+')' if binding.startswith('target.') else 'runtime('+binding+')'
    if kind=='symbol':
        identity=value['value']
        if identity.startswith('@@'):return 'Symbol.'+identity[2:]
        if identity.startswith('@for:'):return 'Symbol.for('+json.dumps(identity[5:],ensure_ascii=False)+')'
        if identity.startswith('@runtime:'):return 'Symbol('+json.dumps(identity[9:],ensure_ascii=False)+')'
        raise ValueError(('unknown symbol identity',identity))
    if kind in ('null','undefined'):return kind
    if kind=='boolean':return str(value['value']).lower()
    if kind=='number_bits':
        v=struct.unpack('>d',bytes.fromhex(value['value']))[0]
        s='NaN'if math.isnan(v) else 'Infinity'if v==math.inf else '-Infinity'if v==-math.inf else repr(v)
        return s
    raise ValueError(('unsupported scalar',value))

def members(d,reference_attrs=5,constant_attrs=0):
    properties={(p['symbol'],p['key']):p for p in d['properties']}
    order=[(False,k)for k in d['own_key_order']]+[(True,k)for k in d['own_symbol_order']]
    if set(order)!=set(properties)or len(order)!=len(properties):raise ValueError('properties disagree with member order')
    edges=set();rows=[]
    for symbol,key in order:
        p=properties[(symbol,key)]
        row=key_text(key,symbol)
        if 'get'in p or'set'in p:
            sides=[]
            for side in ('get','set'):
                if side in p:edges.add(p[side]);sides.append(side+' = '+m.render_path(p[side]))
            row+=' = accessor('+', '.join(sides)+')';default=4
        else:
            value=p['value']
            if value['kind']=='reference':
                target=value['value'];edges.add(target);default=reference_attrs
                if target!=default_target(d['name'],key,symbol):row+=' = ref('+m.render_path(target)+')'
            else:
                row+=' = '+scalar(value);default=4 if symbol and key=='@@toStringTag'else constant_attrs
        if p['attributes']!=default:
            flags=[name for bit,name in [(1,'writable'),(2,'enumerable'),(4,'configurable')]if p['attributes']&bit]
            row+=', attrs = '+' | '.join(flags or ['none'])
        rows.append(row)
    return rows,edges

def render(d):
    rows,edges=members(d)
    if d.get('prototype'):edges.add(d['prototype'])
    facts=[m.render_path(d['name'])]
    if d['kind']!='prototype':facts.append('kind = '+d['kind'])
    if d.get('prototype') and d['prototype']!='Object.prototype':facts.append('extends = '+m.render_path(d['prototype']))
    facts.append('members = { '+'; '.join(rows)+('; ' if rows else '')+'}')
    extra=set(d['births'])-edges
    if extra:facts.append('births = ['+', '.join(m.render_path(e)for e in sorted(extra))+']')
    return ', '.join(facts)

def migrate(root,baseline,output,apply=False):
    families=m.constructor_families(root)
    oracle={d['name']:d for d in baseline['declarations']}
    companions={p:json.loads(p.read_text())for p in root.glob('crates/*/src/**/*.realm.json')}
    ds=[]
    for doc in companions.values():
        for entries in doc['groups'].values():
            for d in entries:
                if d['name'].endswith('.prototype')and d['name'].removesuffix('.prototype')in families and d.get('header')==dict(gc_type=2,reserved=2176,class_id=0)and'function_info'not in d:
                    ds.append(d)
    path=root/'crates/perry-runtime/src/object/global_this/populate.rs'
    source=path.read_text()
    expression='super::proto_room::alloc_builtin_prototype()'
    assert source.count(expression)==1
    offset=source.index(expression);line=source[:offset].count('\n')+1
    declarations='; '.join(render(d)for d in sorted(ds,key=lambda d:d['name']))
    replacement='crate::realm_decl! { birth '+expression+'; realm { '+declarations+'; } }'
    placements=[dict(name=d['name'],source=str(path.relative_to(root)),line=line,old_source='crates/'+oracle[d['name']]['source'],old_line=oracle[d['name']]['line'])for d in ds]
    moved={d['name']for d in ds};deleted=[]
    if apply:
        path.write_text(source.replace(expression,replacement,1))
        for p,doc in companions.items():
            for key,entries in doc['groups'].items():doc['groups'][key]=[d for d in entries if d['name']not in moved]
            if any(doc['groups'].values()):p.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                p.unlink();deleted.append(str(p.relative_to(root)))
                owner=p.with_name(p.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(p.name)+r'#\*"\);[^\n]*$',re.M)
                text,n=pattern.subn('',owner.read_text());assert n==1
                owner.write_text(text.rstrip()+'\n')
    report=dict(placements=placements,entries_moved=len(ds),cross_file_entries=sum(p['source']!=p['old_source']for p in placements),deleted_companions=deleted)
    output.mkdir(parents=True,exist_ok=True);(output/'migration-prototype-births.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k not in ('placements','deleted_companions')})
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1])
    p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
