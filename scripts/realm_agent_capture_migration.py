#!/usr/bin/env python3
"""Bind agent captures to existing owner calls and event-selector rows."""
import argparse
import copy
import json
import struct
from pathlib import Path
import realm_source_migration as m

def classify(d,original):
    if not d.get('body','').split('::')[-1]in ('global_agent_add_request_thunk','global_agent_get_name_thunk','global_agent_listener_thunk'):return None
    owner=d['name'].split('.<prototype>.')[0]
    assert owner in ('node:http.globalAgent','node:https.globalAgent')
    method=d['name'].split('.')[-1]
    c=copy.deepcopy(d);c['captures']=[]
    default=m.default_length(c['function_info']);c['length']=default
    for p in c['properties']:
        if p['key']=='length':p['value']['value']=struct.pack('>d',float(default)).hex()
    human=original(c);assert human is not None,d['name']
    human.pop('births',None)
    if method=='addRequest':binding='raw_ref('+json.dumps(owner)+')'
    else:
        binding='flag_owner(global_agent_prototype, is_https, '+json.dumps(owner)+')'
        if method in ('on','once'):binding+=', flag_property(global_agent_listener_value, once)'
    human['capture']='['+binding+']'
    macro='realm_agent_'+('listener' if method in ('on','once')else method)+'_public'
    human.update(spec='{length = '+str(d['length'])+'}',emit='metadata('+macro+')')
    return human
def migrate(root,baseline,output,apply=False):
    path=root/'crates/perry-runtime/src/object/native_module/namespace_builders.rs'
    oldnew=[
      ('set_builtin_closure_length(closure as usize, 2);','set_builtin_closure_length(closure as usize, realm_agent_listener_public!(length));'),
      ('crate::fn_info!(global_agent_get_name_thunk, 2; with_declared(2)),\n                0,',None),
    ]
    original=m.simple_facts;m.simple_facts=lambda d:classify(d,original)
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==8
    if apply:
        s=path.read_text();old,new=oldnew[0];assert s.count(old)==1;s=s.replace(old,new,1)
        # The annotations occupy the invocation line; change only the following birth argument.
        import re
        for body,length,macro in [('global_agent_get_name_thunk',0,'realm_agent_getName_public'),('global_agent_add_request_thunk',4,'realm_agent_addRequest_public')]:
            pattern=r'('+body+r', [^\n]+\n\s*)'+str(length)+','
            s,n=re.subn(pattern,r'\g<1>'+macro+'!(length),',s,count=1);assert n==1
        path.write_text(s)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
