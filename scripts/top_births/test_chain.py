#!/usr/bin/env python3
"""chain.py follows a cause that is a bare function id (a function without an escape token) to that
function's earliest event, strictly backwards by seq. Pure fixture, no compiler. Red on base: the
chain stopped at `no earlier cause` after the escape."""
import json, sys, tempfile
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import chain
N = 2**32 - 1
rs = [
    dict(record='function', id=7, params=[50, 51], this=N, result=N, rest=N, name='f'),
    dict(record='node', id=50, label='param0', top=True),
    dict(record='node', id=51, label='param1', top=True),
    dict(record='node', id=52, label='later', top=True),
    # f's param 51 went ⊤ at 40, its param 50 at 30 (earliest), a node that arrives after the escape at 150
    dict(record='birth-arrival', node=51, seq=40, source=N, cause=N, call=N, parent=N, injected=False, gate=None),
    dict(record='birth-arrival', node=50, seq=30, source=N, cause=N, call=N, parent=N, injected=True, gate=None),
    dict(record='birth-injection', node=50, injector='x/seed.rs', line=9, why='the seed', source=N, call=N, escape=N, owner=N),
    dict(record='token', id=3, kind='object', name='obj:o', layout=None, function=None, escape='stored'),
    dict(record='escape-first', token=3, reason='stored', cause=dict(function=7), call=None, node=None, seq=100),
]
with tempfile.TemporaryDirectory() as td:
    p = Path(td) / 'f.dump'
    p.write_text(''.join('D ' + json.dumps(r, separators=(",", ":")) + '\n' for r in rs))
    d = chain.load(str(p))
    out = []
    last = chain.chain(d, ('tok', 3), out.append)
    text = '\n'.join(out)
    assert last == ('node', 50), (last, text)
    assert 'seed: seed.rs:9' in text and 'no earlier cause' not in text.split('seed:')[0], text
    seqs = [int(l.split()[0]) for l in out if l[:9].strip().isdigit()]
    assert seqs == sorted(seqs, reverse=True) and seqs == [100, 30], seqs
print('PASS: a bare function-id cause continues to the function\'s earliest earlier event, strictly backwards')
