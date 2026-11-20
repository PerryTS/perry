#!/usr/bin/env python3
"""The census stamps each node's first ⊤ and each token's first escape with one clock; chain.py walks
it back to the seed. Deliberately fails on base (no `birth-arrival` records, no escape `seq`)."""
import argparse, json, os, subprocess, sys, tempfile
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'scripts/top_births'))
import chain
p = argparse.ArgumentParser(); p.add_argument('--perry', required=True); p.add_argument('--runtime', required=True); p.add_argument('--workspace', required=True); a = p.parse_args()
# console.error touches its rest-array argument with an unknown protocol: the array escapes, then its
# element (a V), then V's prototype. Then `this.x =` in the escaped add() stores into ⊤. The chain from
# V's prototype must walk back, by strictly smaller seq, to the array's escape at the console call.
SRC = '''class V { x = 0; constructor(x: number) { this.x = x; } add(b: V) { this.x += b.x; return this; } }
function error(...params: any[]) {
  const message = "THREE." + params.shift();
  console.error(message, ...params);
}
const a = new V(1), b = new V(2);
if (a.x > 5) error("V.add:", a);
a.add(b);
console.log(a.x);
'''
with tempfile.TemporaryDirectory(prefix='birth-arrival-') as td:
    r = Path(td); (r / 'input.ts').write_text(SRC)
    env = dict(os.environ, PERRY_RUNTIME_DIR=a.runtime, PERRY_WORKSPACE_ROOT=a.workspace, PERRY_FACTS_ONLY='1', PERRY_FACTS_UNITS='0', PERRY_ALLOW_PERRY_FEATURES='1')
    for k in ('PERRY_FACTS_EXPLAIN', 'PERRY_FACTS_SUPPRESS_BIRTH', 'PERRY_FACTS_BIRTH_CENSUS'):
        env.pop(k, None)
    def run(name, census):
        dump = r / (name + '.dump')
        e = dict(env, PERRY_FACTS_DUMP=str(dump))
        if census:
            e['PERRY_FACTS_BIRTH_CENSUS'] = '1'
        with (r / (name + '.log')).open('w') as log:
            subprocess.run(['timeout', '180', a.perry, 'compile', '--cache-dir', str(r / ('cache-' + name)), str(r / 'input.ts'), '-o', str(r / ('output-' + name))], env=e, stdout=log, stderr=subprocess.STDOUT, check=True)
        return dump, [json.loads(l[2:]) for l in open(dump) if l.startswith('D {')]
    dump, on = run('on', True)
    arr = [x for x in on if x['record'] == 'birth-arrival']
    assert arr, 'compiler emitted no birth-arrival records'
    esc = [x for x in on if x['record'] == 'escape-first']
    assert esc and all(isinstance(x.get('seq'), int) for x in esc), f'escape-first without seq: {esc[:3]}'
    seqs = [x['seq'] for x in arr + esc] + [x['seq'] for x in on if x['record'] == 'birth-opener']
    assert len(seqs) == len(set(seqs)), 'two census events share a seq'
    by = {x['node']: x['seq'] for x in arr}
    late = [x for x in arr if x['source'] is not None and x['source'] in by and by[x['source']] > x['seq']]
    assert not late, f'a copy source reached ⊤ after its target: {late[:3]}'
    d = chain.load(str(dump))
    lines = []
    ev = chain.start_event(d, 'input.ts:V')
    assert ev[0] == 'tok' and d['tok'][ev[1]]['kind'] == 'prototype', f'V prototype is not escaped: {ev}'
    last = chain.chain(d, ev, lines.append)
    walked = [int(l.split()[0]) for l in lines if l.split()[0].isdigit()]
    assert walked == sorted(walked, reverse=True) and len(set(walked)) == len(walked), f'chain not strictly earlier: {lines}'
    assert last[0] == 'tok' and 'unknown function value' in d['esc'][last[1]]['reason'], \
        'chain does not end at the console call: ' + '\n'.join(lines)
    _, off = run('off', False)
    assert not any(x['record'].startswith('birth-') for x in off), 'census-off dump has birth records'
    assert all(x.get('seq') is None for x in off if x['record'] == 'escape-first'), 'census-off escape-first has a seq'
    print('PASS: one clock over arrivals/escapes/openers, sources before targets, V chain ends at the console call; census off stays silent')
