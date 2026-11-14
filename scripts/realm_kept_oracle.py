"""Read an intrinsic through the fixture's held roots and R1 prototype edges."""
import json, re, hashlib
from collections import deque

def paths_for(names, reach, declarations):
    rows = {row['name']:row for row in declarations}
    paths = {}
    for prop in rows['%Global%'].get('properties', []):
        value = prop.get('value', {})
        if value.get('kind') != 'reference' or prop.get('symbol'):
            continue
        target = value['value']
        if target in names and (target in reach['own_names'] or prop['key'] in reach.get('global_keys', [])):
            paths[target] = ('globalThis[' + json.dumps(prop['key']) + ']', [('%Global%', prop['key'], target)])
    queue = deque(paths)
    while queue:
        name = queue.popleft()
        expr, edges = paths[name]
        row = rows.get(name, {})
        links = []
        if row.get('prototype'):
            links.append((row['prototype'], 'Object.getPrototypeOf(' + expr + ')', '[[Prototype]]'))
        if row.get('constructor') is not None:
            for prop in row.get('properties', []):
                value = prop.get('value', {})
                if prop['key'] == 'prototype' and value.get('kind') == 'reference':
                    links.append((value['value'], '(' + expr + ').prototype', 'prototype'))
        for target, value, key in links:
            if target in names and target not in paths:
                paths[target] = (value, edges + [(name, key, target)])
                queue.append(target)
    return paths

def array_iterator_seed(paths, projection):
    """Use a pristine iterator factory whose member is actually kept."""
    def kept(holder, key):
        return any(p['key'] == key for p in projection.get(holder, {}).get('properties', []))
    if kept('Array.prototype', '@@iterator'):
        return '[][Symbol.iterator]()'
    # A fresh Buffer has the selected typed-array iterator through its declared
    # prototype chain. Reading Array.prototype here would invent a kept slot.
    chain = ['Buffer.prototype', 'Uint8Array.prototype', '%TypedArray%.prototype']
    if ('Buffer' in paths and kept('Buffer', 'alloc')
            and kept('%TypedArray%.prototype', 'values')
            and projection.get('Buffer', {}).get('constructor') is not None
            and all(projection.get(a, {}).get('prototype') == b for a, b in zip(chain, chain[1:]))):
        return '(' + paths['Buffer'][0] + ').alloc(0).values()'
    raise ValueError('no selected pristine Array iterator factory in fixture reach')

def rewrite(source, names, reach, manifest, proof, projection):
    raw = manifest.read_bytes()
    paths = paths_for(names, reach, json.loads(raw)['declarations'])
    applied = {}
    iterator_seed = None
    global_keys = [p['key'] for p in projection['%Global%']['properties'] if not p.get('symbol')]
    text = source.read_text()
    # This recognized expression lowers directly to GlobalThisExpr and the
    # existing js_get_global_this materialize point. It reads no Global alias.
    text = re.sub(r"\bglobalThis\b", "root", text)
    text = text.replace("const root: any = root;", 'const root: any = Function("return this")();')
    if '%ArrayIteratorPrototype%' in names:
        iterator_seed = array_iterator_seed(paths, projection).replace('globalThis', 'root')
        old_seed = 'const arrayIteratorPrototype = () => Object.getPrototypeOf([][Symbol.iterator]());'
        assert text.count(old_seed) == 1
        text = text.replace(old_seed, 'const arrayIteratorPrototype = () => Object.getPrototypeOf(' + iterator_seed + ');')
    def replace(match):
        name = match[1]
        if name not in paths:
            return match[0]
        expr, edges = paths[name]
        expr = expr.replace('globalThis', 'root')
        applied[name] = {'expression':expr, 'edges':edges}
        return 'intrinsic(' + json.dumps(name) + ', () => ' + expr + ');'
    text = re.sub(r'^intrinsic\("([^"]+)", .*\);$', replace, text, flags=re.M)
    old_filter = 'if (cursor === 0 && !inR(name)) continue;'
    assert text.count(old_filter) == 1
    text = text.replace(old_filter, 'if (cursor === 0 && ' + json.dumps(global_keys) + '.indexOf(name) < 0) continue;')
    source.write_text(text)
    proof.write_text(json.dumps({'manifest_sha256':hashlib.sha256(raw).hexdigest(), 'paths':applied, 'global_kept_keys':global_keys, 'array_iterator_seed':iterator_seed}, indent=2)+'\n')
    return source

if __name__ == '__main__':
    # A structure-only constructor is not a Global root. R1 keeps its parent's
    # identity and constructor.prototype; no arbitrary member read is admitted.
    declarations = [
        {'name':'%Global%', 'properties':[{'key':'A','value':{'kind':'reference','value':'A'}}, {'key':'B','value':{'kind':'reference','value':'B'}}]},
        {'name':'A', 'prototype':'B', 'constructor':{}, 'properties':[{'key':'prototype','value':{'kind':'reference','value':'A.prototype'}}]},
        {'name':'B', 'prototype':'Base'}, {'name':'A.prototype','prototype':'Base.prototype'},
    ]
    paths = paths_for({'A','B','Base','A.prototype','Base.prototype'}, {'own_names':['A']}, declarations)
    assert paths['B'][0] == 'Object.getPrototypeOf(globalThis["A"])'
    assert len(paths['Base.prototype'][1]) == 3
    assert 'globalThis["B"]' not in paths['B'][0]
    assert not paths_for({'B'}, {'own_names':[]}, declarations)
    print('kept oracle paths: positive and absent-root controls green')
