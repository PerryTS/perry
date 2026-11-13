#!/usr/bin/env python3
"""LIFE I3: the region ceiling tables from the region TSVs (`PERRY_LIFE_REGIONS`).

Usage: life-regions-report.py --out DIR NAME=SITES:REGIONS[:ALLOC] ...

SITES is the compile-time site file (`PERRY_LIFE_MEASURE=<path>`: `id<TAB>key`, with `body` and `loop` rows),
REGIONS the run's region TSV, ALLOC (optional) the run's census TSV (`PERRY_LIFE_ALLOC`) for the consistency check.

Attribution rule (the headline): each byte sample (the census's byte-proportional draw) carries the verdict of
every region instance open at its object's birth, innermost first. Its bytes count in class
  a: some enclosing loop iteration ended clean;
  b: not a, some enclosing activation ended clean;
  c: neither, its turn ended clean;
  d: no enclosing instance ended clean (an `unresolved` verdict counts as not clean; the share of bytes whose
     chain holds one is reported beside).
Bet columns: the same, counting a clean instance only when its region id is clean in at least 99% (100%) of its
id-sampled non-empty instances. Per region id, bytes are attributed once: to the INNERMOST clean instance of the
chain, and separately to the OUTERMOST clean instance below the turn.
"""
import argparse
from collections import defaultdict
from pathlib import Path

HOLDERS = ["frame", "outer_frame", "shadow", "global", "remembered", "object", "other", "old"]


def load_sites(path):
    keys = {}
    for line in Path(path).read_text().splitlines():
        sid, _, key = line.partition("\t")
        keys[sid] = key.split("\t")
    return keys


def where(keys, kind, rid):
    if kind == "turn":
        return "(turn)"
    k = keys.get(rid)
    if not k:
        return f"? {kind} {rid}"
    if k[0] == "loop":
        return f"loop {k[3]} in {k[2]} ({k[1]})"
    if k[0] == "body":
        return f"body {k[2]} ({k[1]}{', ' + k[3] if len(k) > 3 and k[3] != 'plain' else ''})"
    return " ".join(k)


def load_regions(path):
    rows, bs, totals = [], [], {}
    lines = Path(path).read_text().splitlines()
    head = lines[0].split("\t")
    for line in lines[1:]:
        f = line.split("\t")
        if f[0] == "#bs":
            chain = []
            if len(f) > 2 and f[2]:
                for c in f[2].split(","):
                    k, rid, v = c.split(":")
                    chain.append((k, rid, v))
            bs.append((int(f[1]), chain))
        elif f[0] == "#totals":
            totals = dict(kv.split("=", 1) for kv in f[1:])
        else:
            r = dict(zip(head, f))
            for k in head[2:]:
                r[k] = float(r[k])
            rows.append(r)
    return rows, bs, totals


def clean_share(r):
    n = r["clean"] + r["dirty"] + r["unresolved"]
    return r["clean"] / n if n else None


def headline(rows, bs, interval, bet=None):
    ok = None
    if bet is not None:
        ok = set()
        for r in rows:
            s = clean_share(r)
            if s is not None and s >= bet:
                ok.add((r["kind"], r["id"]))
    out = defaultdict(float)
    for n, chain in bs:
        w = n * interval
        out["total"] += w
        if not chain:
            out["outside"] += w
            continue
        clean = {k: False for k in ("loop", "body", "turn")}
        for k, rid, v in chain:
            if v == "c" and (ok is None or k == "turn" or (k, rid) in ok):
                clean[k] = True
        if any(v == "u" for _, _, v in chain):
            out["has_unresolved"] += w
        cls = "a" if clean["loop"] else "b" if clean["body"] else "c" if clean["turn"] else "d"
        out[cls] += w
    return out


def pct(x, t):
    return f"{100 * x / t:.1f}" if t else "-"


SHARE_BINS = [("0", lambda s: s == 0), ("(0,50]", lambda s: 0 < s <= 0.5), ("(50,90]", lambda s: 0.5 < s <= 0.9),
              ("(90,99)", lambda s: 0.9 < s < 0.99), ("[99,100)", lambda s: 0.99 <= s < 1), ("100", lambda s: s == 1)]
SURV = ["1", "2-4", "5-16", "17+"]


def is_lib(keys, rid):
    """A region id's code is library code when its file lies under `node_modules` (the program's own
    top-level file, the benchmark driver, does not)."""
    k = keys.get(rid)
    return bool(k) and len(k) > 1 and "node_modules/" in k[1]


def share_bin(s):
    return next(name for name, f in SHARE_BINS if f(s))


def analysis(name, keys, rows, bs, interval, acc):
    """The I3 questions from one run (`acc` accumulates the combined rows). Tables:
    driver vs library split of class a; top region ids by innermost-clean bytes with nesting depth;
    bytes by the region id's clean share; survivors of dirty instances; the bound of the restarted loops."""
    by = {(r["kind"], r["id"]): r for r in rows}
    restarted = {r["id"] for r in rows if r["kind"] == "loop" and r.get("restarted", 0) > 0}
    unreg = {sid for sid, k in keys.items() if len(k) > 2 and k[0] in ("loop", "body") and k[2].startswith("<unregistered")}
    out = [f"## {name}", ""]
    split = defaultdict(float)
    inner, depth, bsid, bsclean = defaultdict(float), defaultdict(list), defaultdict(float), defaultdict(float)
    for n, chain in bs:
        w = n * interval
        split["total"] += w
        nt = [(k, rid, v) for k, rid, v in chain if k != "turn"]
        for i, (k, rid, v) in enumerate(nt):
            depth[(k, rid)].append(len(nt) - i)
            bsid[(k, rid)] += w
            if v == "c":
                bsclean[(k, rid)] += w
        if any(k == "body" and rid in unreg for k, rid, _ in nt):
            split["unreg"] += w
            if any(v == "c" for k, _, v in nt if k == "loop"):
                split["unreg_a"] += w
        cl = [(k, rid) for k, rid, v in nt if v == "c" and k == "loop"]
        cb = [(k, rid) for k, rid, v in nt if v == "c"]
        if cb:
            inner[cb[0]] += w
        if not cl:
            continue
        split["a"] += w
        if not is_lib(keys, cl[-1][1]):
            split["a_driver"] += w
            if len(cl) == 1:
                split["a_driver_only"] += w
        else:
            split["a_lib"] += w
            if cl[0] == cl[-1]:
                split["a_lib_single"] += w
        if all(rid in restarted for _, rid in cl):
            split["a_restarted_only"] += w
        if cl[0][1] in restarted:
            split["a_restarted_inner"] += w
    for k, v in split.items():
        acc["split"][k] += v
    t = split["total"]
    out += ["| sampled MB | a | a: outermost clean loop in the driver file | of it: the driver loop is the only clean loop | "
            "a: outermost clean loop in library code | of it: one clean loop only (innermost = outermost) | "
            "a decided only by restarted loops | chain holds an `<unregistered>` body (of it in a) |",
            "|---|---|---|---|---|---|---|---|",
            f"| {t / 1e6:.2f} | {pct(split['a'], t)} | {pct(split['a_driver'], t)} | {pct(split['a_driver_only'], t)} | "
            f"{pct(split['a_lib'], t)} | {pct(split['a_lib_single'], t)} | {pct(split['a_restarted_only'], t)} | "
            f"{pct(split['unreg'], t)} ({pct(split['unreg_a'], t)}) |", ""]
    out += ["Top region ids by innermost-clean bytes (a byte sample's bytes go to the innermost clean instance of its chain):", "",
            "| region | code | depth (median) | instances | decided (id-sampled) | instance clean share | byte-sample clean share (n) | innermost-clean MB |",
            "|---|---|---|---|---|---|---|---|"]
    for key, w in sorted(inner.items(), key=lambda x: -x[1])[:8]:
        r = by.get(key, {})
        s = clean_share(r) if r else None
        d = sorted(depth[key])
        n = int(round(bsid[key] / interval))
        out.append(f"| {where(keys, *key)} | {'library' if is_lib(keys, key[1]) else 'driver'} | {d[len(d) // 2]} | "
                   f"{int(r.get('instances', 0))} | {int(r.get('sampled', 0))} | {'-' if s is None else f'{100 * s:.1f}%'} | "
                   f"{pct(bsclean[key], bsid[key])}% ({n}) | {w / 1e6:.2f} |")
    out.append("")
    # Clean-share distribution: per region id, the instance clean share (decided, non-empty instances) and the
    # byte-sample clean share; bytes are the id's instance bytes (nested ids count the same byte once each).
    dist = defaultdict(lambda: [0, 0.0, [], 0.0])
    for r in rows:
        if r["kind"] == "turn":
            continue
        s = clean_share(r)
        b = share_bin(s) if s is not None else "no decided instance"
        d = dist[(r["kind"], "inst", b)]
        d[0] += 1
        d[1] += r["bytes"]
        d[2].append(int(r["clean"] + r["dirty"] + r["unresolved"]))
        key = (r["kind"], r["id"])
        if bsid.get(key):
            d = dist[(r["kind"], "bs", share_bin(bsclean[key] / bsid[key]))]
            d[0] += 1
            d[1] += bsid[key]
            d[2].append(int(round(bsid[key] / interval)))
    for k, d in dist.items():
        a = acc["dist"][k]
        a[0] += d[0]
        a[1] += d[1]
        a[2] += d[2]
    out += dist_table(dist)
    # Dirty instances: survivors.
    sv = survivors(rows)
    for k, v in sv.items():
        for i, x in enumerate(v):
            acc["surv"][k][i] += x
    out += surv_table(sv)
    return out


def dist_table(dist):
    lines = ["Bytes by the region id's clean share. `inst`: share of its decided non-empty instances, bytes = all bytes "
             "allocated inside its instances; `bs`: share of its byte samples, bytes = sampled bytes inside it. "
             "n = decided instances (inst) or byte samples (bs) per id: min / median.", "",
             "| kind | basis | " + " | ".join(b for b, _ in SHARE_BINS) + " | no decided instance |",
             "|---|---|" + "---|" * (len(SHARE_BINS) + 1)]
    for kind in ("loop", "body"):
        for basis in ("inst", "bs"):
            tot = sum(d[1] for (k, b, _), d in dist.items() if k == kind and b == basis) or 1
            cells = []
            for b in [x for x, _ in SHARE_BINS] + ["no decided instance"]:
                d = dist.get((kind, basis, b))
                if not d or not d[0]:
                    cells.append("-")
                    continue
                ns = sorted(d[2])
                cells.append(f"{100 * d[1] / tot:.1f}% ({d[0]} ids; n {ns[0]} / {ns[len(ns) // 2]})")
            lines.append(f"| {kind} | {basis} | " + " | ".join(cells) + " |")
    return lines + [""]


def survivors(rows):
    """Per kind: dirty instances by survivor count, survivor objects and bytes, holders, and the dirty
    instances' inside bytes split by survivor bucket (exact for ids whose dirty instances are all in one
    bucket, else apportioned by instance count: `appo`)."""
    sv = defaultdict(lambda: [0.0] * 24)
    for r in rows:
        if r["kind"] == "turn" or not r["dirty"]:
            continue
        v = sv[r["kind"]]
        cnt = [r["surv1"], r["surv2_4"], r["surv5_16"], r["surv17"]]
        n = sum(cnt)
        for i in range(4):
            v[i] += cnt[i]
            v[4 + i] += r[["surv_bytes1", "surv_bytes2_4", "surv_bytes5_16", "surv_bytes17"][i]]
            v[8 + i] += r["bytes_w_dirty"] * cnt[i] / n if n else 0
        if sum(1 for c in cnt if c) > 1:
            v[12] += r["bytes_w_dirty"]
        v[13] += r["surv_objs"]
        for j, h in enumerate(HOLDERS):
            v[14 + j] += r["h_" + h]
        v[22] += r["bytes_w_dirty"]
        v[23] += r["bytes_w_clean"]
    return sv


def surv_table(sv):
    lines = ["Dirty instances by surviving objects (1 / 2-4 / 5-16 / 17+). inside MB: bytes allocated inside those "
             "instances (apportioned by instance count for the `appo` share of ids with dirty instances in several "
             "buckets); cumulative: share of dirty inside bytes in instances with <= 1 / <= 4 / <= 16 survivors.", "",
             "| kind | dirty instances | survivor objects | survivor MB | dirty inside MB (clean inside MB) | inside MB by bucket | "
             "cumulative <=1 / <=4 / <=16 | appo share | holders (survivor objects) |",
             "|---|---|---|---|---|---|---|---|---|"]
    for kind in ("loop", "body"):
        v = sv.get(kind)
        if not v:
            continue
        d = v[22] or 1
        cum = [sum(v[8:9]), sum(v[8:10]), sum(v[8:11])]
        ht = sum(v[14:22]) or 1
        lines.append(f"| {kind} | {int(sum(v[0:4]))} ({' / '.join(str(int(x)) for x in v[0:4])}) | {int(v[13])} | "
                     f"{sum(v[4:8]) / 1e6:.2f} | {v[22] / 1e6:.2f} ({v[23] / 1e6:.2f}) | "
                     f"{' / '.join(f'{x / 1e6:.2f}' for x in v[8:12])} | {' / '.join(pct(x, d) for x in cum)} | "
                     f"{pct(v[12], d)} | "
                     + ", ".join(f"{h} {100 * v[14 + j] / ht:.0f}%" for j, h in sorted(enumerate(HOLDERS), key=lambda x: -v[14 + x[0]]) if v[14 + j])
                     + " |")
    return lines + [""]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--top", type=int, default=30)
    ap.add_argument("runs", nargs="+")
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    md = ["# LIFE I3 region ceiling", "", __doc__.split("Attribution rule")[1].join(["Attribution rule", ""]), ""]
    head_rows, comb = [], defaultdict(lambda: defaultdict(float))
    cover = ["| program | site loops / bodies | ran (instances > 0) | zero instances (not run, or no hook) | "
             "run-time ids without a site row | `<unregistered>` site rows | iterations ended by the next start "
             "(no back-edge hook) | empty instances of sampled |",
             "|---|---|---|---|---|---|---|---|"]
    check = ["| program | born MB | before + turns + tail = born | census total - unplaced MB | unresolved instances | "
             "unobserved collections | busy | decisions | forced minors | fulls | probe s / run s |",
             "|---|---|---|---|---|---|---|---|---|---|---|"]
    ana = ["# LIFE I3 region analysis", ""]
    agg = {"split": defaultdict(float), "dist": defaultdict(lambda: [0, 0.0, []]),
           "surv": defaultdict(lambda: [0.0] * 24)}
    for spec in a.runs:
        name, _, rest = spec.partition("=")
        parts = rest.split(":")
        keys = load_sites(parts[0])
        rows, bs, t = load_regions(parts[1])
        interval = int(t.get("interval", "16384"))
        born = int(t["born_bytes"])
        acc = int(t["before_bytes"]) + int(t["turn_bytes"]) + int(t["tail_bytes"])
        census = "-"
        if len(parts) > 2 and Path(parts[2]).exists():
            tot = unplaced = 0
            for line in Path(parts[2]).read_text().splitlines():
                f = line.split("\t")
                if f[0] == "#type":
                    tot += int(f[3]) + int(f[5])
                elif f[0] == "#meta":
                    m = dict(kv.split("=", 1) for kv in f[1:])
                    unplaced = int(m.get("unplaced_bytes", "0"))
            census = f"{(tot - unplaced) / 1e6:.3f} ({tot - unplaced - born - int(t['before_bytes']):+d} B)"
        unres = sum(r["unresolved"] for r in rows)
        sampled = sum(r["sampled"] for r in rows)
        check.append(f"| {name} | {born / 1e6:.2f} | {'yes' if acc == born + int(t['before_bytes']) else 'NO'} "
                     f"({acc} / {born}) | {census} | {int(unres)} of {int(sampled)} | {t['unobserved']} | {t['busy']} | "
                     f"{t['decisions']} | {t['minors']} | {t['fulls']} (unseen {t['fulls_unseen']}) | "
                     f"{int(t['probe_ns']) / 1e9:.1f} / {int(t['elapsed_ns']) / 1e9:.1f} |")
        # Hook coverage: site rows (compile time) against region ids (run time).
        site = {(k[0], sid) for sid, k in keys.items() if k and k[0] in ("loop", "body")}
        ran = {(r["kind"], r["id"]) for r in rows if r["instances"] > 0}
        unknown = [r for r in rows if r["kind"] != "turn" and (r["kind"], r["id"]) not in site]
        unreg = {(k[0], sid) for sid, k in keys.items() if k and k[0] in ("loop", "body") and k[2].startswith("<unregistered")}
        restarted = [r for r in rows if r.get("restarted", 0) > 0]
        cover.append(
            f"| {name} | {sum(1 for k, _ in site if k == 'loop')} / {sum(1 for k, _ in site if k == 'body')} | "
            f"{sum(1 for x in site if x in ran and x[0] == 'loop')} / {sum(1 for x in site if x in ran and x[0] == 'body')} | "
            f"{sum(1 for x in site if x not in ran and x[0] == 'loop')} / {sum(1 for x in site if x not in ran and x[0] == 'body')} | "
            f"{len(unknown)} ({sum(r['bytes'] for r in unknown) / 1e6:.2f} MB) | "
            f"{len(unreg)} ({sum(r['bytes'] for r in rows if (r['kind'], r['id']) in unreg) / 1e6:.2f} MB) | "
            f"{len(restarted)} ids, {int(sum(r['restarted'] for r in restarted))} iterations "
            f"({sum(r['bytes'] for r in restarted) / 1e6:.2f} MB) | "
            f"{int(sum(r['empty'] for r in rows))} of {int(sum(r['sampled'] for r in rows))} |")
        hs = [headline(rows, bs, interval), headline(rows, bs, interval, 0.99), headline(rows, bs, interval, 1.0)]
        for i, h in enumerate(hs):
            for k, v in h.items():
                comb[i][k] += v
        head_rows.append((name, hs))
        # Per region id.
        inner, outer = defaultdict(float), defaultdict(float)
        for n, chain in bs:
            w = n * interval
            first = next(((k, rid) for k, rid, v in chain if v == "c"), None)
            if first:
                inner[first] += w
            last = [(k, rid) for k, rid, v in chain if v == "c" and k != "turn"]
            if last:
                outer[last[-1]] += w
        rows.sort(key=lambda r: -r["bytes"])
        lines = [f"## {name}: top {a.top} region ids by bytes allocated inside", "",
                 "| region | instances | sampled | clean / dirty / unres / empty | clean share | clean bytes share | "
                 "innermost-clean MB | outermost-clean MB | survivors 1 / 2-4 / 5-16 / 17+ | holders |",
                 "|---|---|---|---|---|---|---|---|---|---|"]
        for r in rows[:a.top]:
            key = (r["kind"], r["id"])
            s = clean_share(r)
            nb = r["bytes_w_clean"] + r["bytes_w_dirty"] + r["bytes_w_unresolved"]
            hold = sorted(((r["h_" + h], h) for h in HOLDERS if r["h_" + h]), reverse=True)
            ht = sum(x for x, _ in hold)
            lines.append(
                f"| {where(keys, r['kind'], r['id'])} | {int(r['instances'])} | {int(r['sampled'])} | "
                f"{int(r['clean'])} / {int(r['dirty'])} / {int(r['unresolved'])} / {int(r['empty'])} | "
                f"{'-' if s is None else f'{100 * s:.1f}%'} | {pct(r['bytes_w_clean'], nb)} | "
                f"{inner[key] / 1e6:.2f} | {outer[key] / 1e6:.2f} | "
                f"{int(r['surv1'])} / {int(r['surv2_4'])} / {int(r['surv5_16'])} / {int(r['surv17'])} | "
                + ", ".join(f"{h} {100 * x / ht:.0f}%" for x, h in hold[:3]) + " |")
        (a.out / f"{name}.md").write_text("\n".join(lines) + "\n")
        md += lines + [""]
        ana += analysis(name, keys, rows, bs, interval, agg)
    hl = ["## Headline: share of allocated bytes (byte samples)", "",
          "| program | sampled MB | a: clean iteration | b: clean activation | c: clean turn only | d: none | "
          "chain has unresolved | a/b/c/d at >=99% ids | a/b/c/d at 100% ids |",
          "|---|---|---|---|---|---|---|---|---|"]
    for name, hs in head_rows + [("combined", [comb[0], comb[1], comb[2]])]:
        h, h99, h100 = hs
        t = h["total"]
        hl.append(f"| {name} | {t / 1e6:.2f} | {pct(h['a'], t)} | {pct(h['b'], t)} | {pct(h['c'], t)} | "
                  f"{pct(h['d'] + h['outside'], t)} | {pct(h['has_unresolved'], t)} | "
                  + " / ".join(pct(x[k], t) for x in [h99] for k in "abc") + f" / {pct(h99['d'] + h99['outside'], t)} | "
                  + " / ".join(pct(x[k], t) for x in [h100] for k in "abc") + f" / {pct(h100['d'] + h100['outside'], t)} |")
    text = "\n".join(hl + ["", "## Consistency", ""] + check + ["", "## Hook coverage", ""] + cover) + "\n"
    s = agg["split"]
    t = s["total"]
    ana += ["## combined", "",
            "| sampled MB | a | a: driver outermost | driver loop only | a: library outermost | one clean library loop | "
            "a decided only by restarted loops | `<unregistered>` body in chain (in a) |",
            "|---|---|---|---|---|---|---|---|",
            f"| {t / 1e6:.2f} | {pct(s['a'], t)} | {pct(s['a_driver'], t)} | {pct(s['a_driver_only'], t)} | "
            f"{pct(s['a_lib'], t)} | {pct(s['a_lib_single'], t)} | {pct(s['a_restarted_only'], t)} | "
            f"{pct(s['unreg'], t)} ({pct(s['unreg_a'], t)}) |", ""]
    ana += dist_table(agg["dist"]) + surv_table(agg["surv"])
    (a.out / "ANALYSIS.md").write_text("\n".join(ana) + "\n")
    (a.out / "HEADLINE.md").write_text(text)
    (a.out / "REGIONS.md").write_text("\n".join(md) + "\n" + text)
    print(text)


if __name__ == "__main__":
    main()
