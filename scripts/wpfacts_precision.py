#!/usr/bin/env python3
"""wpfacts precision report (slice 1 go/no-go): how much of the DYNAMIC
upper bound measured by the node census (scratchpad/wpfacts-20261012-
evidence) the STATIC analysis actually proves.

For every census site the census counts as Tier 0 (a mono site on a
prefix-stable template reading a definite own data field, a slot-compatible
set, or a monomorphic method call on a non-escaping template), the static
read/call site(s) on the same source line with the same property are
looked up in the facts dump; the site counts as proven when every matching
static site's verdict is T0/T0set.

Executions are per op (census run B minus run A, divided by the op count),
exactly as analyze.py computes them. Store sites have no source position in
HIR, so the comparison covers reads and method calls; stores are reported
as "unmatched".

Usage: wpfacts_precision.py <name> <facts-dump> <censusA.json|-> <censusB.json> <ops> [--json out]
"""
import collections
import json
import os
import sys
from pathlib import Path

TRACK = {"class", "fnctor", "protoobj", "lit"}
HARD = {"defineProperty", "setPrototypeOf", "freeze", "seal", "preventExtensions",
        "Proxy", "Object.create", "Reflect.construct", "delete"}


# The census ran depth-12 copies of the binary-trees witnesses.
ALIASES = {"arena12.ts": "binarytrees_arena.ts", "plain12.ts": "binarytrees_plain.ts"}


def norm(path):
    p = str(path)
    i = p.rfind("node_modules/")
    if i >= 0:
        return p[i + len("node_modules/"):]
    n = Path(p).name
    return ALIASES.get(n, n)


def census(fa, fb, ops):
    B = json.load(open(fb))
    A = json.load(open(fa)) if fa != "-" else None
    protos = B["protos"]
    pkey = lambda J, pid: (J["protos"][pid]["kind"], J["protos"][pid]["name"],
                           tuple(J["protos"][pid]["chain"]))

    def site_map(J):
        out = collections.defaultdict(collections.Counter)
        for row in J["sites"]:
            k = (norm(J["files"][row[1]]), row[2], row[3], row[4])
            for pid, c in row[7]:
                out[k][pkey(J, pid)] += c
        return out

    SB = site_map(B)
    SA = site_map(A) if A else {}
    esc = collections.defaultdict(collections.Counter)
    for pid, ops_ in B["escapes"]:
        for op, c in ops_:
            esc[pkey(B, pid)][op] += c
    fields = collections.defaultdict(dict)
    for f in B["fields"]:
        k = pkey(B, f["id"])
        d = fields[k].setdefault(f["prop"], dict(slots=set(), r=0, rProto=0, wInit=0,
                                                 wPost=0, addPost=0, kw=0, dl=0))
        for x, y in (("r", "r"), ("rProto", "rProto"), ("wInit", "wInit"), ("wPost", "wPost"),
                     ("addPost", "addPost"), ("kw", "kw"), ("dl", "del")):
            d[x] += f[y]
        d["slots"] |= set(f.get("slots", []))

    def prefix_stable(k):
        if k[0] not in TRACK:
            return False
        if any(op in HARD for op in esc.get(k, {})):
            return False
        return not any(d["dl"] for d in fields.get(k, {}).values())

    def definite(k, p):
        d = fields.get(k, {}).get(p)
        return d is not None and d["addPost"] == 0 and d["kw"] == 0

    def own(k, p):
        d = fields.get(k, {}).get(p)
        return d is not None and d["r"] + d["wInit"] + d["wPost"] > 0 and d["rProto"] == 0

    t0 = {}
    for k, cnt in SB.items():
        fname, line, kind, prop = k
        if kind not in ("get", "call", "set", "rmw"):
            continue
        prev = SA.get(k, collections.Counter())
        n = sum((c - prev.get(pk, 0)) for pk, c in cnt.items()) / ops
        if n <= 0:
            continue
        recv = [pk for pk in cnt if cnt[pk] > 0]
        if len(recv) == 1 and recv[0][0] in TRACK:
            pk = recv[0]
            if not prefix_stable(pk):
                continue
            if kind == "call":
                t0[k] = (n, "call")
            elif own(pk, prop) and definite(pk, prop):
                t0[k] = (n, "mono")
        else:
            trk = [pk for pk in recv if pk[0] in TRACK]
            if kind != "call" and len(trk) > 1 and len(trk) == len(recv):
                ok = all(prefix_stable(pk) and own(pk, prop) and definite(pk, prop)
                         and len(fields[pk][prop]["slots"]) == 1 for pk in trk)
                if ok and len({next(iter(fields[pk][prop]["slots"])) for pk in trk}) == 1:
                    t0[k] = (n, "set")
    return t0


def static_sites(dump):
    mods, sites = {}, collections.defaultdict(list)
    line_tab = {}
    # CJS modules are parsed wrapped (`W <module> <prefix lines> <file>`):
    # offsets count in the wrapped text, lines shift by the prefix.
    wrapped = {}
    for l in open(dump):
        if l.startswith("W "):
            _, i, pre, f = l.rstrip("\n").split(" ", 3)
            wrapped[int(i)] = (int(pre), f)
    for l in open(dump):
        if l.startswith("M "):
            _, i, p = l.rstrip("\n").split(" ", 2)
            mods[int(i)] = p
        elif l.startswith("S "):
            q = l.rstrip("\n").split(" ")
            m, off, key, call = int(q[1]), int(q[2]), q[3][1:], q[4] == "call=1"
            verdict = l.rstrip("\n").rsplit("verdict=", 1)[1]
            path = mods[m]
            pre, text = wrapped.get(m, (0, path))
            if path not in line_tab:
                try:
                    data = Path(text).read_bytes()
                    starts = [0] + [i + 1 for i, b in enumerate(data) if b == 10]
                except OSError:
                    starts = None
                line_tab[path] = starts
            starts = line_tab[path]
            if not starts:
                continue
            import bisect
            line = bisect.bisect_right(starts, off) - pre
            sites[(norm(path), line, "call" if call else "get", key)].append(verdict)
    return sites


def stem(f):
    """`effect/dist/SchemaAST.js` and `effect/src/SchemaAST.ts` -> `effect/SchemaAST`."""
    parts = [p for p in f.split("/") if p not in ("dist", "src", "esm", "cjs", "lib")]
    last = parts[-1].rsplit(".", 1)[0] if parts else f
    return "/".join(parts[:1] + [last])


def reason_class(v):
    if v in ("T0", "T0set"):
        return "proven"
    r = v[3:] if v.startswith("no:") else v
    if r.startswith("escape: "):
        return "escape: " + r[8:][:90]
    if r.startswith("hard: "):
        return "hard: " + r[6:][:90]
    if r.startswith("chain: "):
        return "chain: " + r[7:][:90]
    if r.startswith("receiver from unmodeled code (TOP) <- "):
        return "TOP <- " + r[len("receiver from unmodeled code (TOP) <- "):][:150]
    return r[:100]


def main():
    name, dump, fa, fb, ops = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], float(sys.argv[5])
    out_json = sys.argv[sys.argv.index("--json") + 1] if "--json" in sys.argv else None
    t0 = census(fa, fb, ops)
    st = static_sites(dump)
    tot = collections.Counter()
    prov = collections.Counter()
    nsites = collections.Counter()
    psites = collections.Counter()
    why = collections.Counter()
    unmatched = collections.Counter()
    stem_matched = collections.Counter()
    # A census taken on a package's built JS while perry compiled its
    # sources (effect: dist/*.js vs src/*.ts) cannot match by line: match
    # by (file stem, property, kind) instead, proven only when EVERY static
    # site of that property in that file is proven.
    static_files = {k[0] for k in st}
    by_stem = collections.defaultdict(list)
    for (f, line, kind, prop), vs in st.items():
        by_stem[(stem(f), kind, prop)].extend(vs)
    for k, (n, tier) in t0.items():
        fname, line, kind, prop = k
        if kind in ("set", "rmw"):
            unmatched["store"] += n
            continue
        kk = (fname, line, "call" if kind == "call" else "get", prop)
        if fname in static_files:
            vs = st.get(kk)
        else:
            vs = by_stem.get((stem(fname), kk[2], prop))
            if vs:
                stem_matched[tier] += n
        tot[tier] += n
        nsites[tier] += 1
        if not vs:
            unmatched["no static site on that line"] += n
            why["(no static site matched)"] += n
            continue
        if all(v in ("T0", "T0set") for v in vs):
            prov[tier] += n
            psites[tier] += 1
        else:
            bad = [v for v in vs if v not in ("T0", "T0set")]
            why[reason_class(bad[0])] += n
    T = sum(tot.values())
    P = sum(prov.values())
    res = dict(name=name, dyn_t0_execs=T, proven_execs=P,
               share=(P / T if T else 0.0),
               by_tier={t: dict(dyn=tot[t], proven=prov[t], sites=nsites[t], proven_sites=psites[t])
                        for t in tot},
               unmatched=dict(unmatched),
               matched_by_file_stem=dict(stem_matched),
               not_proven_by_reason=dict(why.most_common(25)))
    print(f"== {name}: dynamic T0 read/call executions/op {T:.1f}, statically proven {P:.1f} "
          f"({100 * res['share']:.1f}%)")
    for t in tot:
        print(f"   {t:5s}: {prov[t]:.1f}/{tot[t]:.1f} execs/op, {psites[t]}/{nsites[t]} sites")
    print(f"   unmatched: {dict((k, round(v, 1)) for k, v in unmatched.items())}")
    if stem_matched:
        print(f"   matched by file stem (census on built JS): {dict((k, round(v, 1)) for k, v in stem_matched.items())}")
    print("   not proven, by reason (execs/op):")
    for r, n in why.most_common(int(os.environ.get("PREC_TOP", "12"))):
        print(f"     {n:12.1f}  {r}")
    if out_json:
        json.dump(res, open(out_json, "w"), indent=1)


if __name__ == "__main__":
    main()
