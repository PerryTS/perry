#!/usr/bin/env python3
"""Compare realm function identity claims with compiled object equality.

The realm supplies the property probes independently of the generated table.
Dropping an alias row therefore changes a claim, never the set of probes.
"""
from __future__ import annotations
import argparse
from collections import defaultdict
import itertools
import json
import os
from pathlib import Path
import sys
from behave import compile_run


def references(realm):
    functions = {d['name'] for d in realm['declarations']
                 if d['kind'] in ('method', 'function', 'constructor', 'getter', 'setter')}
    out = {}
    for d in realm['declarations']:
        if not d['name'].endswith('.prototype'):
            continue
        for p in d.get('properties', []):
            v = p.get('value', {})
            if v.get('kind') == 'reference' and v.get('value') in functions:
                out[d['name'], p['key']] = v['value']
    return out


def claims(path):
    out = {}
    for line in path.read_text().splitlines():
        row = line.split('\t')
        if row[0] == 'native':
            _, owner, key, identity = row
            out[owner, key] = identity
    return out


def expression(path, modules):
    owner, key = path
    if owner.startswith('node:'):
        module, _, holder = owner.rpartition('.')
        # Namespace holder can have a .prototype suffix.
        module, holder = owner.split('.', 1)
        base = modules[module]
        for part in holder.split('.'):
            base += f'[{json.dumps(part)}]'
    elif owner.startswith('%TypedArray%'):
        base = 'Object.getPrototypeOf(Uint8Array.prototype)'
    else:
        base = 'globalThis'
        for part in owner.split('.'):
            base += f'[{json.dumps(part)}]'
    return base + (f'[Symbol.{key[2:]}]' if key.startswith('@@') else f'[{json.dumps(key)}]')


def pairs(refs):
    groups = defaultdict(list)
    for path, identity in refs.items():
        groups[identity].append(path)
    alias = sorted(p for group in groups.values() for p in itertools.combinations(sorted(group), 2))
    different = []
    paths = sorted({p for pair in alias for p in pair})
    for a, b in zip(paths, paths[1:]):
        if refs[a] != refs[b]:
            different.append((a, b))
        if len(different) == 12:
            break
    return alias, different


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--perry', required=True)
    ap.add_argument('--realm', type=Path, required=True)
    ap.add_argument('--table', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    a = ap.parse_args(argv)
    refs = references(json.loads(a.realm.read_text()))
    table = claims(a.table)
    same, different = pairs(refs)
    assert same and different, 'identity probes must exercise both verdicts'
    a.out.mkdir(parents=True, exist_ok=True)
    modules = {owner.split('.', 1)[0]: 'realm_module_' + str(n)
               for n, owner in enumerate(sorted({owner for owner, _ in refs if owner.startswith('node:')}))}
    program = [f'import * as {var} from {json.dumps(module)};' for module, var in modules.items()]
    expected = []
    for n, (left, right) in enumerate(same + different):
        l, r = expression(left, modules), expression(right, modules)
        # Missing rows claim distinct identities, so a weakened table fails.
        equal = table.get(left, left) == table.get(right, right)
        label = f'EV pair {n} '
        program.append(f'try {{ const a:any={l}; const b:any={r}; console.log({json.dumps(label)} + (typeof a === "function" && typeof b === "function") + " " + (a === b)); }} catch(e) {{ console.log({json.dumps(label + "error ")} + e); }}')
        expected.append(label + 'true ' + str(equal).lower())
    program.append("const f:any=[].map;\nconst s:any=''.slice;\nf.cxidTag=1;\nconsole.log('EV fields disjoint ' + s.cxidTag);\nconsole.log('EV fields same ' + ([].map as any).cxidTag);\nconst x:any=new Set();\nx.keys.cxidTag=1;\nconsole.log('EV fields alias ' + x.values.cxidTag);\nconst m:any=new Map();\nconsole.log('EV inherited identity ' + (m.hasOwnProperty === Object.prototype.hasOwnProperty));\nconst push:any=[].push;\nconst pop:any=[].pop;\nconst slice:any=[].slice;\npush.cxidTag=1; pop.cxidTag=2;\nconsole.log('EV fields push-pop-slice ' + slice.cxidTag);\nconst probe:any={field:1};\nm.hasOwnProperty({toString(){probe.field=2;return 'x';}});\nconsole.log('EV key conversion ' + probe.field);\n")
    source = a.out / 'function-identity.ts'
    source.write_text('\n'.join(program) + '\n')
    text, err = compile_run(a.perry, source, dict(os.environ, PERRY_NO_CACHE='1'))
    source.with_suffix('.log').write_text(text)
    red = [event for event in expected if text.splitlines().count(event) != 1]
    if err:
        print('RED', err)
    for event in red:
        n = int(event.split()[2]); left, right = (same + different)[n]
        print(f'RED {left} / {right}: expected {event}')
    for event in ('EV fields disjoint undefined', 'EV fields same 1', 'EV fields alias 1',
                  'EV inherited identity true', 'EV fields push-pop-slice undefined', 'EV key conversion 2'):
        if text.splitlines().count(event) != 1:
            red.append(event)
            print('RED expected exactly once:', event)
    print(f'alias_pairs={len(same)} different_pairs={len(different)} controls=6 red={len(red)}')
    return int(bool(err or red))


if __name__ == '__main__':
    sys.exit(main())
