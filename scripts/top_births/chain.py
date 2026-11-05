#!/usr/bin/env python3
"""chain.py DUMP(.gz) START...: follow a census cause chain back in time to its seed.

START is `tok:N` (a token's first escape), `node:N` (a node's first ⊤), `gate:HOLDER:KEY` (KEY `*` =
every key) or a bare token name suffix such as `three.core.js:Vector3` (its prototype token).

Every step is an event stamped by the census clock (`seq`): a gate opener (`birth-opener`), a node's
first ⊤ (`birth-arrival`) or a token's first escape (`escape-first`). From an event the chain moves to
the EARLIEST recorded cause whose seq is smaller, so it always ends, never in a loop: at an injection
(the seed: its injector file:line and why), at a ⊤ seeded before the solve, or at an event whose
causes are all later (printed as `no earlier cause`).

Causes of each event kind:
- escape of token T: the escaping parent token, the rule's node (its first ⊤), the callee node of an
  unknown call `[call N]`, the function T's escape was caused by (that function's token's escape).
- first ⊤ of node N: its copy source's first ⊤; for an injection, the gate it replays (that gate's
  opener), the escaping parent token, the rule's node.
- gate opener: the receiver node's first ⊤, the escaping parent token, the rule's node."""
import gzip, json, re, sys

NONE = (None, 4294967295)

def load(path):
    op = gzip.open if path.endswith('.gz') else open
    d = {'arr': {}, 'esc': {}, 'gate': {}, 'tok': {}, 'fn_tok': {}, 'uc': {}, 'inj': {}, 'label': {}, 'fn': {}, 'fn_uc': {}}
    with op(path, 'rb') as f:
        for l in f:
            if not l.startswith(b'D {'):
                continue
            k = l[3:60]
            if b'"record":"birth-arrival"' in k:
                r = json.loads(l[2:]); d['arr'][r['node']] = r
            elif b'"record":"escape-first"' in k:
                r = json.loads(l[2:]); d['esc'][r['token']] = r
            elif b'"record":"birth-opener"' in k:
                r = json.loads(l[2:]); d['gate'][(r['holder'], r['key'])] = r
            elif b'"record":"token"' in k:
                r = json.loads(l[2:]); d['tok'][r['id']] = r
                if r['kind'] == 'function' and r.get('function') is not None:
                    d['fn_tok'].setdefault(r['function'], r['id'])
            elif b'"record":"unknown-call"' in k:
                r = json.loads(l[2:]); d['uc'][r['call']] = r
                if r.get('rootFunction') not in NONE: d['fn_uc'].setdefault(r['rootFunction'], []).append(r)
            elif b'"record":"function"' in k:
                r = json.loads(l[2:]); d['fn'][r['id']] = r
            elif b'"record":"birth-injection"' in k:
                r = json.loads(l[2:]); d['inj'].setdefault(r['node'], []).append(r)
            elif b'"record":"node"' in k:
                r = json.loads(l[2:]); d['label'][r['id']] = r.get('label', '')
    return d

def seq_of(d, ev):
    kind, x = ev
    r = {'node': d['arr'], 'tok': d['esc'], 'gate': d['gate']}[kind].get(x)
    return None if r is None else r.get('seq')

def gate_for(d, holder, key):
    # a read of KEY passes the gate (holder, KEY) and the every-key gate (holder, None): the earlier one.
    gs = [g for g in ((holder, key), (holder, None)) if g in d['gate']]
    return min(gs, key=lambda g: d['gate'][g]['seq']) if gs else None

def fn_events(d, f):
    """The events that made function F matter: its token's first escape, the first ⊤ of its
    this/params/result/rest nodes (receiver and callee operands) and of the callee node of each
    unknown call in it. The caller keeps only events earlier than the one it came from, so the walk
    stays strictly backwards."""
    out = []
    if d['fn_tok'].get(f) in d['esc']: out.append(('tok', d['fn_tok'][f]))
    r = d['fn'].get(f, {})
    ns = [r.get('this'), r.get('result'), r.get('rest')] + list(r.get('params') or [])
    ns += [u.get('node') for u in d['fn_uc'].get(f, [])]
    out += [('node', n) for n in ns if isinstance(n, int) and n not in NONE and n in d['arr']]
    return out

def causes(d, ev):
    kind, x = ev
    out = []
    if kind == 'tok':
        e = d['esc'][x]; c = e.get('cause') or {}
        if 'token' in c: out.append(('tok', c['token']))
        if e.get('node') not in NONE: out.append(('node', e['node']))
        m = re.search(r'\[call (\d+)\]', e.get('reason') or '')
        u = d['uc'].get(int(m.group(1))) if m else None
        if u and u.get('node') not in NONE: out.append(('node', u['node']))
        if 'function' in c: out += fn_events(d, c['function'])
    elif kind == 'node':
        a = d['arr'][x]
        if a.get('source') not in NONE: out.append(('node', a['source']))
        g = a.get('gate')
        if g:
            gg = gate_for(d, g['token'], g['key'])
            if gg: out.append(('gate', gg))
        if a.get('parent') not in NONE: out.append(('tok', a['parent']))
        if a.get('cause') not in NONE: out.append(('node', a['cause']))
    else:
        o = d['gate'][x]
        if o.get('recv') not in NONE: out.append(('node', o['recv']))
        if o.get('parent') not in NONE: out.append(('tok', o['parent']))
        if o.get('cause') not in NONE: out.append(('node', o['cause']))
    return [c for c in out if c != ev]

def describe(d, ev):
    kind, x = ev
    if kind == 'tok':
        t = d['tok'].get(x, {}); e = d['esc'][x]
        return f"escape tok {x} {t.get('kind', '?')} {t.get('name', '?')} | {e.get('reason', '')[:140]}"
    if kind == 'node':
        a = d['arr'][x]
        how = 'injected' if a.get('injected') else ('copy from %s' % a['source'] if a.get('source') not in NONE else 'rule')
        return f"top node {x} {d['label'].get(x, '')[:90]} | {how}"
    o = d['gate'][x]
    return (f"opener {o['holderName']}[{'*' if o['key'] is None else o['key']}] {o['what']} | "
            f"site {o['site']} {o['siteFunction']} {o['siteDesc'][:80]} recv {o['recv']}")

def chain(d, ev, out=print):
    """Prints the chain from ev; returns the last event (the seed or the break point)."""
    while True:
        s = seq_of(d, ev)
        out('%8s  %s' % (s, describe(d, ev)))
        if ev[0] == 'node' and d['arr'][ev[1]].get('injected'):
            for r in d['inj'].get(ev[1], []):
                out('          seed: %s:%s %s' % (r['injector'].split('/')[-1], r['line'], r['why'][:160]))
        earlier = [(seq_of(d, c), c) for c in causes(d, ev)]
        earlier = [(cs, c) for cs, c in earlier if cs is not None and s is not None and cs < s]
        if not earlier:
            out('          (no earlier cause)')
            return ev
        ev = min(earlier)[1]

def start_event(d, a):
    if a.startswith('tok:'): return ('tok', int(a[4:]))
    if a.startswith('node:'): return ('node', int(a[5:]))
    if a.startswith('gate:'):
        _, h, k = a.split(':', 2)
        return ('gate', (int(h), None if k == '*' else k))
    toks = [t for t, r in d['tok'].items() if r['name'].endswith(a) and t in d['esc']]
    toks.sort(key=lambda t: (d['tok'][t]['kind'] != 'prototype', t))
    if not toks:
        raise SystemExit(f'{a}: no escaped token with that name')
    return ('tok', toks[0])

def main():
    d = load(sys.argv[1])
    for a in sys.argv[2:]:
        print('===', a)
        chain(d, start_event(d, a))

if __name__ == '__main__':
    main()
