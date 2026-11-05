#!/usr/bin/env python3
"""The census names the store that FIRST opened a builtin-prototype gate; deliberately fails on base."""
import argparse, json, os, subprocess, tempfile
from pathlib import Path
p = argparse.ArgumentParser(); p.add_argument('--perry', required=True); p.add_argument('--runtime', required=True); p.add_argument('--workspace', required=True); a = p.parse_args()
# `t[k] = 1` stores into the unknown value (⊤) under a computed key: it opens every key. `u.w = 2`
# stores under `w`: it opens `w` only if it runs before the computed store (an opened every-key
# gate covers `w`), so its row is optional, but when present it names second() and comes first.
SRC = '''declare function sink(x: any): any;
const o: any = JSON.parse('{"a":1}');
function first(k: string) { const t: any = sink(o); t[k] = 1; }
function second() { const u: any = sink(o); u.w = 2; }
first(String(Date.now()));
second();
class V { x = 0; add(b: V) { this.x += b.x; return this; } }
const v = new V().add(new V());
console.log(v.x, ({} as any).w);
'''
with tempfile.TemporaryDirectory(prefix='birth-opener-') as td:
    r = Path(td); (r / 'input.ts').write_text(SRC)
    env = dict(os.environ, PERRY_RUNTIME_DIR=a.runtime, PERRY_WORKSPACE_ROOT=a.workspace, PERRY_FACTS_ONLY='1', PERRY_FACTS_UNITS='0', PERRY_ALLOW_PERRY_FEATURES='1')
    for k in ('PERRY_FACTS_EXPLAIN', 'PERRY_FACTS_SUPPRESS_BIRTH', 'PERRY_FACTS_BIRTH_CENSUS'):
        env.pop(k, None)
    def run(name, census):
        e = dict(env, PERRY_FACTS_DUMP=str(r / (name + '.dump')))
        if census:
            e['PERRY_FACTS_BIRTH_CENSUS'] = '1'
        with (r / (name + '.log')).open('w') as log:
            subprocess.run(['timeout', '180', a.perry, 'compile', '--cache-dir', str(r / ('cache-' + name)), str(r / 'input.ts'), '-o', str(r / ('output-' + name))], env=e, stdout=log, stderr=subprocess.STDOUT, check=True)
        return [json.loads(l[2:]) for l in open(r / (name + '.dump')) if l.startswith('D {')]
    on = run('on', True)
    ops = sorted((x for x in on if x['record'] == 'birth-opener'), key=lambda x: x['seq'])
    assert ops, 'compiler emitted no birth-opener records'
    assert len({x['seq'] for x in ops}) == len(ops), 'two openers share a sequence number'
    assert len({(x['holder'], x['key']) for x in ops}) == len(ops), 'a gate has two openers'
    any_top = [x for x in ops if x['key'] is None and x["holderName"] == "TOP"]
    assert any_top, f'no every-key opener on TOP: {ops}'
    o = any_top[0]
    assert o["siteFunction"].endswith(":first"), f'every-key gate names {o["siteFunction"]!r}, not the computed store in first(): {o}'
    assert o['recv'] is not None and o['recvRoot'] is not None and o['recvWhy'], f'opener names no receiver chain: {o}'
    w = [x for x in ops if x['key'] == 'w' and x["holderName"] == "TOP"]
    assert all(x["siteFunction"].endswith(":second") and x['seq'] < o['seq'] for x in w), f'named gate w: {w}'
    nodes = {x['id'] for x in on if x['record'] == 'node'}
    assert o['recv'] in nodes, 'the receiver node record is not dumped'
    off = run('off', False)
    assert not any(x['record'].startswith('birth-') for x in off), 'census-off dump has birth records'
    print('PASS: first opener per gate (computed store in first(), named store in second()), receiver chain, census-off absence')
