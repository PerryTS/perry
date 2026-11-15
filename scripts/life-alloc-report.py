#!/usr/bin/env python3
"""LIFE I2 step A tables from PERRY_LIFE_ALLOC TSVs.

usage: life-alloc-report.py [--syms <runtime-syms.txt>] --prog <name> <sites> <tsv> [--prog ...] --out <dir>
The alloc TSV is <tsv minus .tsv>.alloc.tsv, the binary <dir of tsv>/<name>.m
(compiled with PERRY_KEEP_SYMBOLS=1). Raw return addresses are resolved here,
offline, with atos against the image base the run logged.

Frame classes come from what the symbols ARE, not from their names:
  runtime   = defined in the runtime archives the binary links
              (`nm -U -j libperry_runtime_instr.a libperry_stdlib_instr.a`,
              the --syms file; a staticlib carries std/core/hashbrown too);
  plumbing  = runtime frames of the allocator and the sampler itself
              (module perry_runtime::arena, capture_ips): never an answer;
  compiled  = inside the binary but not defined by the runtime archives
              (what the compiler emitted for the program: *landing_*, module
              inits, the program's `main`);
  system    = outside the binary image (atos cannot name it: libsystem).
Entry point = the frame directly below (callee side of) the first compiled
frame. Allocator-side function = the innermost runtime (non-plumbing) frame.
"""
import collections, math, os, re, subprocess, sys

args = sys.argv[1:]
progs, out, syms_path = [], None, 'runtime-syms.txt'
i = 0
while i < len(args):
    if args[i] == '--prog':
        progs.append((args[i + 1], args[i + 2], args[i + 3])); i += 4
    elif args[i] == '--out':
        out = args[i + 1]; i += 2
    elif args[i] == '--syms':
        syms_path = args[i + 1]; i += 2
    else:
        sys.exit(f'bad arg {args[i]}')
os.makedirs(out, exist_ok=True)
RT = set(l.rstrip('\n') for l in open(syms_path))
PLUMB_RE = re.compile(r'13perry_runtime5arena|capture_ips|perry_runtime::arena::')
DEMANGLED_RT = ('perry_runtime::', 'perry_stdlib::', 'std::', 'core::', 'alloc::', 'hashbrown::', '<')
method = collections.Counter()


def clean(name):
    name = re.sub(r'\.llvm\.\d+$', '', name)
    return re.sub(r'::h[0-9a-f]{16}$', '', name)


def classify(name):
    if name.startswith('0x'):
        return 'system'
    if name in RT:
        method['nm'] += 1
        return 'plumbing' if PLUMB_RE.search(name) else 'runtime'
    if name.startswith(DEMANGLED_RT):
        method['demangled-prefix'] += 1
        return 'plumbing' if PLUMB_RE.search(name) else 'runtime'
    method['not-in-runtime'] += 1
    return 'compiled'


def parse(path):
    meta, coll, arenas, types, unnoted, samples = {}, {}, {}, {}, {}, []
    for line in open(path):
        f = line.rstrip('\n').split('\t')
        if f[0] == '#meta':
            meta = dict(x.split('=', 1) for x in f[1:])
        elif f[0] == '#collector':
            coll = {k: int(v) for k, v in (x.split('=', 1) for x in f[1:])}
        elif f[0] == '#arenas':
            arenas = {k: int(v) for k, v in (x.split('=', 1) for x in f[1:])}
        elif f[0] == '#type':
            types[f[1]] = [int(x) for x in f[2:8]]
        elif f[0] == '#unnoted':
            unnoted[f[1]] = [int(f[2]), int(f[3])]
        elif f[0] == 'sample':
            samples.append((f[1], f[2], f[3], int(f[4]), int(f[5]), [int(x, 16) for x in f[6:]]))
    return meta, coll, arenas, types, unnoted, samples


def symbolize(binary, base, pcs):
    pcs = sorted(set(pcs))
    res = {}
    for k in range(0, len(pcs), 2000):
        chunk = pcs[k:k + 2000]
        r = subprocess.run(['atos', '-o', binary, '-l', hex(base)] + [hex(p - 1) for p in chunk],
                           capture_output=True, text=True)
        for p, l in zip(chunk, r.stdout.splitlines()):
            name = re.sub(r' \(in [^)]*\)( \+ \d+)?( \([^)]*\))?$', '', l.strip())
            res[p] = clean(name)
    return res


def attribute(kind, pcs, sym):
    """(entry point, allocator-side function, first compiled frame)."""
    names = [sym.get(p, hex(p)) for p in pcs]
    cls = [classify(n) for n in names]
    comp = next((i for i, c in enumerate(cls) if c == 'compiled'), None)
    compiled = names[comp] if comp is not None else '(no compiled frame within 12)'
    upto = comp if comp is not None else len(names)
    alloc_fn = next((names[i] for i in range(upto) if cls[i] == 'runtime'), None)
    if kind == 'inline':
        return '(inline bump, compiled code)', '(inline bump)', compiled
    if comp is not None and comp > 0:
        entry = names[comp - 1]
    elif comp == 0:
        entry = '(compiled frame at depth 0)'
    else:
        rts = [n for n, c in zip(names, cls) if c == 'runtime']
        entry = 'deep: ' + (rts[-1] if rts else '?')
    return entry, alloc_fn or entry, compiled


def pct(a, b):
    return f'{100.0 * a / b:.1f}' if b else '-'


def mb(x):
    return f'{x / 1e6:.2f}'


def demangle_v0(n):
    """Readable path of a Rust v0 symbol: the identifiers in order."""
    if not n.startswith('_R'):
        return n
    ids = []
    s, k = n, 2
    while k < len(s):
        m = re.match(r'(\d+)', s[k:])
        if not m:
            k += 1; continue
        ln = int(m.group(1)); k += len(m.group(1))
        if s[k:k + 1] == '_':
            k += 1
        ident = s[k:k + ln]; k += ln
        if ident and not ident.startswith(('perry_runtime', 'perry_stdlib')):
            ids.append(ident)
    return '::'.join(ids) if ids else n


def nice(n):
    return demangle_v0(n)[:100]


comb_types = collections.defaultdict(lambda: [0] * 6)
C = collections.Counter()
comb = {k: collections.Counter() for k in ('entry', 'comp', 'alloc', 'kind', 'state')}
comb_entry_alloc = collections.defaultdict(collections.Counter)
comb_entry_types = collections.defaultdict(collections.Counter)
comb_type_alloc = collections.defaultdict(collections.Counter)
comb_type_entry = collections.defaultdict(collections.Counter)
summary, recon, agree = [], [], []
names_seen = collections.Counter()
interval = None

for name, _sites, tsv in progs:
    apath = tsv[:-4] + '.alloc.tsv'
    binary = os.path.join(os.path.dirname(tsv), name + '.m')
    if not os.path.exists(apath):
        print(f'{name}: no {apath}'); continue
    meta, coll, arenas, types, unnoted, samples = parse(apath)
    interval = int(meta['interval'])
    sym = symbolize(binary, int(meta['image_base'], 16), [p for s in samples for p in s[5]])
    for s in samples:
        for p in s[5]:
            n = sym.get(p, hex(p))
            names_seen[(classify(n), n)] += s[3]
    rt = sum(v[1] for v in types.values()); il = sum(v[3] for v in types.values()); cv = sum(v[5] for v in types.values())
    total = rt + il
    covered_by_type = {t: v[5] for t, v in types.items()}
    # reconciliation
    arena = int(meta['arena_bytes'])
    arena_sum = sum(arenas.get(k, 0) for k in ('young', 'old', 'longlived', 'survivor0', 'survivor1'))
    armed = arenas.get('armed_arena_bytes', 0); evac = arenas.get('evacuated_bytes', 0); refused = arenas.get('refused_bytes', 0)
    copied = coll.get('copied_bytes', 0); promoted = coll.get('promoted_bytes', 0); inplace = coll.get('in_place_promoted_bytes', 0)
    unn = sum(v[1] for v in unnoted.values())
    expect = arena - armed - copied - (promoted - inplace) - evac
    resid = expect - total
    recon.append((name, arena, arena_sum, armed, copied, promoted - inplace, evac, refused, unn, expect, total, resid, arenas))
    for t, v in types.items():
        for k in range(6):
            comb_types[t][k] += v[k]
    C['total'] += total; C['covered'] += cv; C['inline'] += il; C['expect'] += expect
    # samples
    st = collections.Counter(); ent = collections.Counter(); kinds = collections.Counter()
    unc_bytes_samples = 0
    lines = [f'# {name}: allocated bytes by type x covered (exact)\n\n',
             f'total {mb(total)} MB (runtime path {mb(rt)}, inline bump {mb(il)}); covered {mb(cv)} MB = {pct(cv, total)}%\n\n',
             f'Reconciliation: arena {arena} (per arena {arenas}); − armed {armed} − copied {copied} − copying promotions {promoted - inplace} − evacuated {evac} = {expect}; census {total}; residue {resid} ({pct(resid, total)}%); refused (double notes removed) {refused}; unnoted old/holder births counted {unn} {unnoted}\n\n',
             '| type | count | bytes MB | runtime MB | inline MB | covered count | covered MB | uncovered MB | covered % |\n|---|---|---|---|---|---|---|---|---|\n']
    for t, v in sorted(types.items(), key=lambda kv: -(kv[1][1] + kv[1][3])):
        b = v[1] + v[3]
        lines.append(f'| {t} | {v[0] + v[2]} | {mb(b)} | {mb(v[1])} | {mb(v[3])} | {v[4]} | {mb(v[5])} | {mb(b - v[5])} | {pct(v[5], b)} |\n')
    for state, kind, t, n, nbytes, pcs in samples:
        if state == 'unplaced':
            # one predicate: an object no path placed never enters the epoch map, so it is never covered
            state = 'uncovered'; st[('unplaced', kind)] += n
        st[(state, kind)] += n
        comb['state'][(state, kind)] += n
        if state != 'uncovered':
            continue
        e, a, c = attribute(kind, pcs, sym)
        ent[e] += n; kinds[kind] += n
        comb['entry'][e] += n; comb['comp'][c] += n; comb['alloc'][a] += n; comb['kind'][kind] += n
        comb_entry_alloc[e][a] += n; comb_entry_types[e][t] += n
        comb_type_alloc[t][a] += n; comb_type_entry[t][e] += n
    nu = sum(ent.values()); nun = sum(v for (s_, k), v in st.items() if s_ == 'unplaced')
    ncov = sum(v for (s_, k), v in st.items() if s_ == 'covered')
    exact_unc = total - cv
    agree.append((name, exact_unc, nu, nu * interval, math.sqrt(nu) * interval, nun, ncov + nu, total, meta.get('inline_slow_objects', '-'), meta.get('inline_slow_bad', '-'), cv, ncov, f"{meta.get('claims_outside', '-')} of them repeat {meta.get('claims_repeat', '-')} ({mb(int(meta.get('claims_repeat_bytes', 0)))} MB)", int(meta.get('claims_outside_bytes', 0)), int(meta.get('unplaced_bytes', 0))))
    lines.append(f'\nSample states: ' + ', '.join(f'{a}/{b}={v}' for (a, b), v in sorted(st.items())) + '\n')
    lines.append(f'\nUncovered: exact {mb(exact_unc)} MB; sampled {nu} x {interval} B = {mb(nu * interval)} MB (± {mb(math.sqrt(nu) * interval)} MB, 1σ); unplaced samples of partly covered types {nun}\n')
    lines.append(f'Inline vs runtime (sampled uncovered): inline {pct(kinds["inline"], nu)}%, runtime {pct(kinds["runtime"], nu)}%\n')
    open(os.path.join(out, f'{name}.alloc.md'), 'w').write(''.join(lines))
    summary.append((name, total, il, cv))

L = ['# LIFE I2 A: allocated bytes by type x covered, who allocates the uncovered part\n\n',
     f'An object of b bytes carries b // {interval} samples plus one with probability (b mod {interval}) / {interval} (order-free, per object); samples x {interval} B estimate bytes. Covered = claimed by a census birth while in the epoch map (allocated since the last collection, first claim), for the exact count and the samples alike. Frame classes by symbol definition (nm of the runtime archives), see the header of i2-work/alloc.py. Classification lookups: {dict(method)}.\n\n',
     '## Per program: exact census, sampled agreement\n\n',
     '| program | allocated MB | inline bump MB | covered % | covered exact MB | covered sampled MB | uncovered exact MB | uncovered sampled MB (±1σ) | (sampled − exact) / σ | uncovered samples (of them unplaced) | all samples x interval MB | claims outside the epoch map (MB) | unplaced exact MB | inline slow-call objects (header mismatch) |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n']
for (name, total, il, cv), (_, eu, nu, su, sg, nun, ns_all, _t, so, sb, cv2, ncov, co, cob, unpb) in zip(summary, agree):
    z = (su - eu) / sg if sg else float('nan')
    L.append(f'| {name} | {mb(total)} | {mb(il)} | {pct(cv, total)} | {mb(cv)} | {mb(ncov * interval)} | {mb(eu)} | {mb(su)} ± {mb(sg)} | {z:+.2f} | {nu} ({nun}) | {mb(ns_all * interval)} | {co} ({mb(cob)}) | {mb(unpb)} | {so} ({sb}) |\n')
L.append('\n## Reconciliation with the collector counters\n\nexpected fresh bytes = Σ arena allocated_bytes (young+old+longlived+survivors) − bytes already allocated when the knob armed − survivor copies − copying promotions (promoted − in place) − old-page evacuations; residue = expected − census total.\n\n')
L.append('| program | arena MB | armed MB | copied MB | copying promotions MB | evacuated MB | expected MB | census MB | residue MB | residue % | refused (removed double notes) MB | unnoted old births MB |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n')
for (name, arena, arena_sum, armed, copied, cprom, evac, refused, unn, expect, total, resid, arenas) in recon:
    L.append(f'| {name} | {mb(arena)} | {mb(armed)} | {mb(copied)} | {mb(cprom)} | {mb(evac)} | {mb(expect)} | {mb(total)} | {mb(resid)} | {pct(resid, total)} | {mb(refused)} | {mb(unn)} |\n')
T = C['total']
L.append(f'\n## Combined: bytes by type x covered (exact)\n\ntotal {mb(T)} MB, covered {pct(C["covered"], T)}%, inline bump {mb(C["inline"])} MB\n\n')
L.append('| type | count | bytes MB | inline MB | covered MB | uncovered MB | covered % | share of all uncovered % |\n|---|---|---|---|---|---|---|---|\n')
unc_all = sum(max(0, v[1] + v[3] - v[5]) for v in comb_types.values())
for t, v in sorted(comb_types.items(), key=lambda kv: -(kv[1][1] + kv[1][3])):
    b = v[1] + v[3]; u = b - v[5]
    L.append(f'| {t} | {v[0] + v[2]} | {mb(b)} | {mb(v[3])} | {mb(v[5])} | {mb(u)} | {pct(v[5], b)} | {pct(max(u, 0), unc_all)} |\n')
ns = sum(comb['entry'].values())
L.append(f'\n## Combined: who allocates the uncovered bytes (sampled, {ns} samples ≈ {mb(ns * interval)} MB)\n\n'
         f'Inline bump in compiled code: {pct(comb["kind"]["inline"], ns)}%; inside runtime entry points: {pct(comb["kind"]["runtime"], ns)}%.\n'
         'Sample states: ' + ', '.join(f'{a}/{b}={v}' for (a, b), v in sorted(comb['state'].items())) + '\n\n')
L.append('| entry point (called by compiled code) | samples | ≈ MB | share % | allocator-side function(s) | types |\n|---|---|---|---|---|---|\n')
for e, v in comb['entry'].most_common(25):
    al = ', '.join(f'{nice(a)} {pct(c, v)}%' for a, c in comb_entry_alloc[e].most_common(2))
    tt = ', '.join(f'{t} {pct(c, v)}%' for t, c in comb_entry_types[e].most_common(3))
    L.append(f'| `{nice(e)}` | {v} | {mb(v * interval)} | {pct(v, ns)} | {al} | {tt} |\n')
L.append('\n## Combined: allocator-side function (innermost runtime frame), uncovered samples\n\n| function | samples | share % |\n|---|---|---|\n')
for a, v in comb['alloc'].most_common(20):
    L.append(f'| `{nice(a)}` | {v} | {pct(v, ns)} |\n')
for t in ('string', 'array', 'object_meta', 'object'):
    tot = sum(comb_type_entry[t].values())
    if not tot:
        continue
    L.append(f'\n## Combined: uncovered {t} ({tot} samples)\n\n| entry point | samples | share % | | allocator-side function | samples | share % |\n|---|---|---|---|---|---|---|\n')
    E = comb_type_entry[t].most_common(15); A = comb_type_alloc[t].most_common(15)
    for k in range(max(len(E), len(A))):
        e = E[k] if k < len(E) else ('', 0); a = A[k] if k < len(A) else ('', 0)
        L.append(f'| `{nice(e[0])}` | {e[1] or ""} | {pct(e[1], tot) if e[1] else ""} | | `{nice(a[0])}` | {a[1] or ""} | {pct(a[1], tot) if a[1] else ""} |\n')
L.append('\n## Combined: first compiled frame (top 20)\n\n| frame | samples | share % |\n|---|---|---|\n')
for c, v in comb['comp'].most_common(20):
    L.append(f'| `{nice(c)}` | {v} | {pct(v, ns)} |\n')
open(os.path.join(out, 'ALLOC.md'), 'w').write(''.join(L))
with open(os.path.join(out, 'alloc-frame-classes.txt'), 'w') as f:
    f.write('# samples\tclass\tsymbol (class: runtime/plumbing = defined in the runtime archives; compiled = in the binary, not in them; system = outside the image)\n')
    for (c, n), v in names_seen.most_common(400):
        f.write(f'{v}\t{c}\t{n[:160]}\n')
print(''.join(L[:30]))
