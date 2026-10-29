#!/usr/bin/env python3
"""Move FFI body facts onto js_function_info!, projecting nondefault lengths locally."""
import argparse
import copy
import json
import re
from pathlib import Path
import realm_source_migration as m

def rewrite(source,declarations):
    edits=[];placements=[]
    for d in declarations:
        suffix=d['name'].split('BlockList.prototype.',1)[1]
        role='getter' if suffix.endswith('.<get>') else 'method'
        key=suffix.removesuffix('.<get>');body=d['body'].rsplit('::',1)[1]
        pattern=re.compile(r'proto\.'+role+r'\(\s*"'+re.escape(key)+r'",\s*(perry_ffi::js_function_info!\('+re.escape(body)+r', (\d+); with_flags\(perry_ffi::FN_BUILTIN\)\))(?P<tail>,\s*(\d+))?,\s*\);')
        matches=list(pattern.finditer(source));assert len(matches)==1,(key,len(matches))
        match=matches[0];old=match[0]
        candidate=copy.deepcopy(d)
        abi=m.default_length(d['function_info'])
        candidate['length']=abi
        for prop in candidate['properties']:
            if prop['key']=='length':prop['value']['value']=m.struct.pack('>d',abi).hex()
        facts=m.simple_facts(candidate);assert facts=={},(key,facts)
        if d['length']==abi:
            replacement=old.replace('js_function_info!(','js_function_info!(#[realm('+m.render_path(d['name'])+')] ',1)
        else:
            assert role=='method' and int(match[4])==d['length']
            lines=old.splitlines();assert len(lines)==5,lines
            metadata='realm_'+body+'_metadata'
            info=match[1][:-1]+'; bind realm_info, '+metadata+')'
            info=info.replace('js_function_info!(','js_function_info!(#[realm('+m.render_path(d['name'])+', spec = { length = '+str(d['length'])+' }, emit = metadata)] ',1)
            indent=re.match(r'\s*',lines[2])[0]
            lines[0]='{'
            lines[1]=indent+'// Public length and body metadata share this declaration.'
            lines[2]=indent+info+';'
            lines[3]=indent+'proto.method('+json.dumps(key)+', realm_info, '+metadata+'!(length));'
            lines[4]=re.match(r'\s*',lines[4])[0]+'}'
            replacement='\n'.join(lines)
        assert old.count('\n')==replacement.count('\n')
        edits.append((match.start(),match.end(),replacement))
        placements.append(dict(name=d['name'],line=source[:match.start(1)].count('\n')+1))
    for a,b,replacement in sorted(edits,reverse=True):source=source[:a]+replacement+source[b:]
    source=source.replace('pub(super) unsafe extern "C" fn install_block_list(', '#[rustfmt::skip] pub(super) unsafe extern "C" fn install_block_list(',1)
    return source,placements

def migrate(root,baseline,output,apply=False):
    m.CONSTANTS=m.abi_constants(root);m.FAMILIES=m.constructor_families(root)
    oracle={d['name']:d for d in baseline['declarations']}
    companion=root/'crates/perry-ext-net/src/classes.realm.json';doc=json.loads(companion.read_text())
    ds=[d for rows in doc['groups'].values()for d in rows if d.get('body','').startswith('perry_ext_net::classes::prototype::')]
    file='crates/perry-ext-net/src/classes_prototype.rs';path=root/file
    source,placements=rewrite(path.read_text(),ds)
    for p in placements:p.update(source=file,old_source='crates/'+oracle[p['name']]['source'],old_line=oracle[p['name']]['line'])
    if apply:
        path.write_text(source);moved={p['name']for p in placements}
        for group,rows in doc['groups'].items():doc['groups'][group]=[d for d in rows if d['name']not in moved]
        if any(doc['groups'].values()):
            companion.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
        else:
            companion.unlink()
            owner=root/'crates/perry-ext-net/src/classes.rs'
            text=owner.read_text()
            suffix='// Source-only realm facts beside the owning builtin bodies.\nmacro_rules! realm_decl {\n    ($facts:literal) => {};\n}\nrealm_decl!("classes.realm.json#*");\n'
            assert text.endswith(suffix)
            owner.write_text(text[:-len(suffix)].rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True)
    (output/'migration-ffi-info.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(placements)),indent=2)+'\n')
    print('FFI body declarations moved',len(placements))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
