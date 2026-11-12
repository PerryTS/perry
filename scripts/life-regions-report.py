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
    (a.out / "HEADLINE.md").write_text(text)
    (a.out / "REGIONS.md").write_text("\n".join(md) + "\n" + text)
    print(text)


if __name__ == "__main__":
    main()
