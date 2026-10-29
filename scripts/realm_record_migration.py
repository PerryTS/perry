#!/usr/bin/env python3
"""Own existing fixed-record builders without changing their scope or lines."""
import argparse
import collections
import json
import re
from pathlib import Path
import realm_source_migration as m
import realm_prototype_birth_migration as p

def locate(source,layout):
    pattern=re.compile(r'crate::object::runtime_layouts::runtime_record!\((\w+), '+re.escape(layout)+r'\);')
    matches=list(pattern.finditer(source))
    assert len(matches)==1,(layout,len(matches))
    return matches[0]

def migrate(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={f:json.loads(f.read_text())for f in root.glob('crates/*/src/**/*.realm.json')}
    pending={d['name']:d for doc in companions.values()for rows in doc['groups'].values()for d in rows}
    base='crates/perry-runtime/src/'
    groups=[(base+'process/metadata_values.rs',layout,name)for layout,name in [
        ('RECORD_Q457_0','process.release'),('RECORD_Q458_0','process.features'),
        ('RECORD_Q468_0','process.config'),('RECORD_Q468_1','process.config.variables'),
        ('RECORD_Q468_2','process.config.target_defaults'),
        ('RECORD_Q468_3','process.config.target_defaults.configurations')]]
    groups += [(base+'process/finalization.rs','RECORD_Q440_0','process.finalization'),
               (base+'process/report.rs','RECORD_Q459_0','process.report'),
               (base+'os.rs','RECORD_F411_0','process.versions')]
    edits=collections.defaultdict(list);placements=[]
    for file,layout,name in groups:
        names=[n for n in (name,'node:'+name)if n in pending]
        if not names:continue
        source=(root/file).read_text();match=locate(source,layout)
        for n in names:assert pending[n]['header']==dict(gc_type=2,reserved=512,class_id=0)
        replacement='crate::realm_decl! { record '+match[1]+', '+layout+'; realm { '+'; '.join(p.render(pending[n])for n in names)+'; } };'
        edits[file].append((match.start(),match.end(),replacement))
        for n in names:
            placements.append(dict(name=n,source=file,line=source[:match.start()].count('\n')+1,old_source='crates/'+oracle[n]['source'],old_line=oracle[n]['line']))
    if apply:
        for file,rs in edits.items():
            path=root/file;source=path.read_text();before=source.count('\n')
            for a,b,replacement in sorted(rs,reverse=True):source=source[:a]+replacement+source[b:]
            assert source.count('\n')==before;path.write_text(source)
        moved={d['name']for d in placements}
        for path,doc in companions.items():
            for group,rows in doc['groups'].items():doc['groups'][group]=[d for d in rows if d['name']not in moved]
            if any(doc['groups'].values()):path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                path.unlink();owner=path.with_name(path.name.replace('.realm.json','.rs'))
                pattern=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(path.name)+r'#\*"\);[^\n]*$',re.M)
                s,n=pattern.subn('',owner.read_text());assert n==1;owner.write_text(s.rstrip()+'\n')
    output.mkdir(parents=True,exist_ok=True)
    (output/'migration-records.json').write_text(json.dumps(dict(placements=placements,entries_moved=len(placements)),indent=2)+'\n')
    print('fixed record births moved',len(placements))
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--apply',action='store_true')
    a=parser.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
