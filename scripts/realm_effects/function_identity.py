#!/usr/bin/env python3
"""Compare realm function identity claims with compiled object equality.

The realm supplies the property probes independently of the generated table.
Dropping an alias row therefore changes a claim, never the set of probes.
"""
from __future__ import annotations
import argparse
from collections import Counter, defaultdict
import itertools
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import subprocess
from behave import compile_run


def references(realm):
    path = Path(__file__).resolve().parents[1] / "wpfacts_realm_declared_keys.py"
    spec = importlib.util.spec_from_file_location("declared_keys", path)
    generated = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generated)
    return generated.rows(realm)


def claims(path):
    out = {}
    for line in path.read_text().splitlines():
        row = line.split('\t')
        if row[0] == 'native':
            _, owner, key, identity = row
            out[owner, key] = identity
    return out


def holder_expression(owner, modules):
    if owner.startswith('node:'):
        module, _, holder = owner.partition('.')
        # Namespace holder can have a .prototype suffix.
        base = modules[module]
        for part in filter(None, holder.split('.')):
            base += f'[{json.dumps(part)}]'
    elif owner == '%Global%':
        base = 'globalThis'
    elif owner.startswith('%TypedArray%'):
        base = 'Object.getPrototypeOf(Uint8Array.prototype)'
    else:
        base = 'globalThis'
        for part in owner.split('.'):
            base += f'[{json.dumps(part)}]'
    return base


def key_expression(key):
    return f"Symbol.{key[2:]}" if key.startswith("@@") else json.dumps(key)


def expression(path, modules):
    owner, key = path
    return holder_expression(owner, modules) + f"[{key_expression(key)}]"


def pairs(refs):
    groups = defaultdict(list)
    for path, identity in refs.items():
        groups[identity].append(path)
    alias = [p for _, group in sorted(groups.items()) for p in itertools.combinations(sorted(group), 2)]
    different = []
    paths = sorted({p for pair in alias for p in pair})
    for a, b in zip(paths, paths[1:]):
        if refs[a] != refs[b]:
            different.append((a, b))
        if len(different) == 12:
            break
    return alias, different


CONTROL_SOURCE="const f:any=[].map;\nconst s:any=''.slice;\nf.cxidTag=1;\nconsole.log('EV fields disjoint ' + s.cxidTag);\nconsole.log('EV fields same ' + ([].map as any).cxidTag);\nconst x:any=new Set();\nx.keys.cxidTag=1;\nconsole.log('EV fields alias ' + x.values.cxidTag);\nconst m:any=new Map();\nconsole.log('EV inherited identity ' + (m.hasOwnProperty === Object.prototype.hasOwnProperty));\nconst push:any=[].push;\nconst pop:any=[].pop;\nconst slice:any=[].slice;\npush.cxidTag=1; pop.cxidTag=2;\nconsole.log('EV fields push-pop-slice ' + slice.cxidTag);\nconst probe:any={field:1};\nm.hasOwnProperty({toString(){probe.field=2;return 'x';}});\nconsole.log('EV key conversion ' + probe.field);\n"


def imports(paths):
    names=sorted({owner.split('.', 1)[0] for owner, _ in paths if owner.startswith('node:')})
    modules={name:f'realm_module_{n}' for n,name in enumerate(names)}
    return modules,[f'import * as {var} from {json.dumps(module)};' for module,var in modules.items()]


def check_program(a, name, program, expected):
    source=a.out/(name+'.ts')
    source.write_text('\n'.join(program)+'\n')
    text,err=compile_run(a.perry,source,dict(os.environ,PERRY_NO_CACHE='1'))
    source.with_suffix('.log').write_text(text)
    observed=Counter(text.splitlines())
    missing=[event for event in expected if observed[event]!=1]
    extra=sum(n for event,n in observed.items() if event.startswith('EV pair '))-sum(event.startswith('EV pair ') for event in expected)
    if err: print('RED',name,err)
    for event in missing[:5]: print('RED',name,'expected exactly once:',event)
    if extra: print('RED',name,'pair count difference',extra)
    return len(missing)+int(bool(err))+int(bool(extra))


def main(argv=None):
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--perry',required=True)
    ap.add_argument('--realm',type=Path,required=True)
    ap.add_argument('--table',type=Path,required=True)
    ap.add_argument('--out',type=Path,required=True)
    a=ap.parse_args(argv)
    rows=references(json.loads(a.realm.read_text()))
    refs={(r[1],r[2]):r[3] for r in rows if r[0]=='native'}
    getters=[r for r in rows if r[0]=='getter-this']
    table=claims(a.table)
    realm_same,different=pairs(refs)
    grouped=defaultdict(list)
    for path in sorted(refs):
        grouped[table.get(path,'missing:'+json.dumps(path))].append(path)
    groups=[(identity,paths) for identity,paths in sorted(grouped.items()) if len(paths)>1]
    total=sum(len(paths)*(len(paths)-1)//2 for _,paths in groups)
    if total!=len(realm_same) or set(table)!=set(refs):
        print(f'RED table coverage: claimed_alias_pairs={total} realm_alias_pairs={len(realm_same)} missing_rows={len(set(refs)-set(table))} extra_rows={len(set(table)-set(refs))}')
        print(f'programs=0 alias_pairs={total} expected_pairs={len(realm_same)} different_pairs={len(different)} controls=8 getters={len(getters)} red=1')
        return 1
    a.out.mkdir(parents=True,exist_ok=True)
    red=0;checked=0;programs=0
    # Each program is exactly one table identity group, including every pair.
    # The realm's independent pair count prevents omitted rows shrinking the check.
    for identity,paths in groups:
        modules,program=imports(paths)
        program.append('const functions:any[]=['+',\n'.join(expression(path,modules) for path in paths)+'];')
        n=len(paths)*(len(paths)-1)//2
        program.append(f"let pair={checked}; for(let i=0;i<functions.length;i++) {{ for(let j=i+1;j<functions.length;j++) {{ const a:any=functions[i]; const b:any=functions[j]; console.log('EV pair '+pair+' '+(typeof a === 'function' && typeof b === 'function')+' '+(a === b)); pair++; }} }}")
        expected=[f'EV pair {k} true true' for k in range(checked,checked+n)]
        name='identity-'+hashlib.sha256(identity.encode()).hexdigest()[:16]
        red+=check_program(a,name,program,expected);checked+=n;programs+=1
    assert checked==total==len(realm_same),'alias pair total drift'
    modules,program=imports([path for pair in different for path in pair])
    expected=[]
    for n,(left,right) in enumerate(different):
        equal=table[left]==table[right]
        label=f'EV different {n} '
        program.append(f"{{ const a:any={expression(left,modules)}; const b:any={expression(right,modules)}; console.log({json.dumps(label)}+(typeof a === 'function' && typeof b === 'function')+' '+(a === b)); }}")
        expected.append(label+'true '+str(equal).lower())
    for n,(_,owner,key,_) in enumerate(getters):
        holder=holder_expression(owner,modules);k=key_expression(key);label=f'EV getter {n} '
        program.append(f'try {{ const marker:any={{}}; let owner:any={holder}; let desc:any=Object.getOwnPropertyDescriptor(owner,{k}); while(desc === undefined && owner !== null) {{ owner=Object.getPrototypeOf(owner); if(owner !== null) desc=Object.getOwnPropertyDescriptor(owner,{k}); }} console.log({json.dumps(label)}+(desc.get.call(marker) === marker)); }} catch(e) {{ console.log({json.dumps(label+"error ")}+e); }}')
        expected.append(label+'true')
    program.append(CONTROL_SOURCE)
    expected += ['EV fields disjoint undefined','EV fields same 1','EV fields alias 1','EV inherited identity true','EV fields push-pop-slice undefined','EV key conversion 2']
    nullable="const cxidF=Date.toString; const cxidA=Date.valueOf; cxidF.cxidAbsent={field:1}; try { console.log('EV nullable date unexpected '+cxidA.cxidAbsent.field); } catch(e) { console.log('EV nullable date caught'); }\nconst cxidK1=process.argv[2]??'map'; const cxidK2=process.argv[3]??'slice'; const cxidU=[1][cxidK1]; const cxidV=[1][cxidK2]; cxidU.cxidAbsent={field:1}; try { console.log('EV nullable anonymous unexpected '+cxidV.cxidAbsent.field); } catch(e) { console.log('EV nullable anonymous caught'); }"
    node=subprocess.run([os.environ.get('NODE_BIN','node'),'-e',nullable],capture_output=True,text=True,timeout=30)
    nullable_expected=['EV nullable date caught','EV nullable anonymous caught']
    assert node.returncode==0 and node.stdout.splitlines()==nullable_expected,'Node nullable controls failed'
    program.append(nullable);expected+=nullable_expected
    red+=check_program(a,'different-getters-controls',program,expected);programs+=1
    print(f'programs={programs} alias_pairs={checked} expected_pairs={len(realm_same)} different_pairs={len(different)} controls=8 getters={len(getters)} red={red}')
    return int(bool(red))


if __name__=='__main__':
    sys.exit(main())
