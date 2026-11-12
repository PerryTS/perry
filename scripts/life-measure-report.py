#!/usr/bin/env python3
"""LIFE I1: join the lifetime measurement with the census and write the tables.

Inputs per program (--prog NAME SITES CENSUS MEASURE, repeatable): the
compile-time site file (`id<TAB>module<TAB>function<TAB>kind<TAB>class<TAB>offset`),
the census TSV (PERRY_LIFE_CENSUS) and the run's measurement TSV
(PERRY_LIFE_MEASURE at run time). Joined by the site key (module, function,
kind, class, offset). Writes one markdown file per program and a combined
summary into --out.

Per site and boundary the estimate is births x dead/decided samples (bytes:
bytes x dead/decided sampled bytes). A boundary that applies to no sample of a
site (no loop, no caller) is `n/a` for it; shares are over the births of the
sites with decided samples at that boundary, with that coverage stated.
"""
import argparse
import math
from collections import defaultdict
from pathlib import Path

B = ["iteration", "return", "caller", "turn"]
HOLDERS = ["frame", "outer_frame", "shadow", "global", "remembered", "object", "other"]


def wilson(a, n, z=1.96):
    if n == 0:
        return (0.0, 1.0)
    p = a / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def load(sites, census, measure):
    keys = {}
    for line in Path(sites).read_text().splitlines():
        sid, key = line.split("\t", 1)
        keys[sid] = tuple(key.split("\t"))
    side = {}
    if census and Path(census).exists():
        for line in Path(census).read_text().splitlines():
            c = line.split("\t")
            if len(c) < 10:
                continue
            module, function, cls, offset, _slots, verdict, _detail, kind, receiving, _outer = c[:10]
            s = receiving if receiving != "-" else "local:" + verdict
            side[(module, function, kind, cls, offset)] = ":".join(s.split(":")[:2])
    lines = Path(measure).read_text().splitlines()
    head = lines[0].split("\t")
    sites_ = {}
    totals = {}
    for line in lines[1:]:
        if line.startswith("#totals"):
            for f in line.split("\t")[1:]:
                k, v = f.split("=", 1)
                totals[k] = int(v)
            continue
        if line.startswith("#"):
            continue
        r = dict(zip(head, line.split("\t")))
        key = keys.get(r["site"], ("?", "?", "?", "?", r["site"]))
        s = sites_.setdefault(r["site"], {
            "key": key, "kind": key[2], "side": side.get(key, "?"), "births": int(r["births"]),
            "bytes": int(r["bytes"]), "unborn": int(r["unborn"]), "sampled": int(r["sampled"]), "b": {}})
        s["b"][r["boundary"]] = {k: float(r[k]) for k in head[6:]}
    return list(sites_.values()), totals


def shares(sites):
    """Per boundary: (births of decided sites, est dead births, bytes decided, est dead bytes,
    samples, unresolved, late)."""
    out = {}
    for b in B:
        nb = db = ny = dy = smp = unr = late = 0.0
        for s in sites:
            d = s["b"].get(b)
            if not d:
                continue
            unr += d["unresolved"]
            n = d["samples"]
            if n == 0:
                continue
            smp += n
            late += d["late"]
            dead_frac = (n - d["alive"]) / n
            wb = d["alive_bytes_w"] + d["dead_bytes_w"]
            dead_bytes_frac = d["dead_bytes_w"] / wb if wb else dead_frac
            nb += s["births"]
            db += s["births"] * dead_frac
            ny += s["bytes"]
            dy += s["bytes"] * dead_bytes_frac
        out[b] = (nb, db, ny, dy, smp, unr, late)
    return out


def pct(a, b):
    return f"{100 * a / b:.1f}%" if b else "-"


def table(rows_by_group, total_births, total_bytes):
    lines = ["| group | births | bytes | " + " | ".join(f"dead@{b[:4]} births / bytes (cover)" for b in B) + " |",
             "|---|---|---|" + "---|" * len(B)]
    for g, sites in rows_by_group:
        births = sum(s["births"] for s in sites)
        bytes_ = sum(s["bytes"] for s in sites)
        sh = shares(sites)
        cells = []
        for b in B:
            nb, db, ny, dy, *_ = sh[b]
            cells.append(f"{pct(db, nb)} / {pct(dy, ny)} ({pct(nb, births)})")
        lines.append(f"| {g} | {births:,} | {bytes_:,} | " + " | ".join(cells) + " |")
    return lines


def holders(sites, b):
    h = defaultdict(float)
    for s in sites:
        d = s["b"].get(b)
        if d:
            for k in HOLDERS:
                h[k] += d[k]
    tot = sum(h.values())
    return ", ".join(f"{k} {pct(h[k], tot)}" for k in HOLDERS if h[k]) + f" (n={int(tot)})"


def report(name, sites, totals):
    md = [f"# {name}", ""]
    births = sum(s["births"] for s in sites)
    bytes_ = sum(s["bytes"] for s in sites)
    sampled = sum(s["sampled"] for s in sites)
    md.append(f"Sites {len(sites)}, births {births:,}, bytes {bytes_:,}, sampled {sampled:,}, unborn (scalar values at a birth) "
              f"{sum(s['unborn'] for s in sites):,}.")
    arena = totals.get("arena_bytes", 0) + totals.get("large_bytes", 0)
    md.append(f"Covered bytes {bytes_:,} of the collector's allocated {arena:,} ({pct(bytes_, arena)}); uncovered "
              f"{pct(arena - bytes_, arena)} (runtime births: strings, builtin results, cells). Probes {totals.get('probes', 0):,}, "
              f"probe minors {totals.get('probe_minors', 0):,}, turn fulls {totals.get('turn_fulls', 0)}, late pops "
              f"{totals.get('late_pops', 0):,}, probe time {totals.get('probe_ns', 0) / 1e9:.1f}s of {totals.get('elapsed_ns', 0) / 1e9:.1f}s, "
              f"budget shift {totals.get('global_shift', 0)}, pending at end {totals.get('pending', 0)}.")
    md.append("")
    md.append("## Table 1: share dead at each boundary, by kind (cover = births of sites decided there)")
    by_kind = defaultdict(list)
    for s in sites:
        by_kind[s["kind"]].append(s)
    md += table([("ALL", sites)] + sorted(by_kind.items()), births, bytes_)
    sh = shares(sites)
    md.append("")
    md.append("Decided samples / unresolved / late per boundary: " + "; ".join(
        f"{b} {int(sh[b][4]):,} / {int(sh[b][5]):,} / {int(sh[b][6]):,}" for b in B))
    md.append("")
    md.append("## Table 1b: by kind x static receiving side (top 25 groups by bytes)")
    by_side = defaultdict(list)
    for s in sites:
        by_side[f"{s['kind']} {s['side']}"].append(s)
    groups = sorted(by_side.items(), key=lambda kv: -sum(s["bytes"] for s in kv[1]))[:25]
    md += table(groups, births, bytes_)
    md.append("")
    md.append("## Holders of survivors (first reacher's walk phase)")
    for b in B:
        md.append(f"- {b}: {holders(sites, b)}")
    md.append("")
    md.append("## Table 2: top 30 sites by bytes")
    md.append("| site (function kind class offset) | side | births | bytes | dead@iter | dead@ret | dead@caller | dead@turn | first dead | CI width (worst) | iter/ret holder |")
    md.append("|---|---|---|---|---|---|---|---|---|---|---|")
    mono_bad = 0
    for s in sites:
        di = s["b"].get("iteration")
        dr = s["b"].get("return")
        if di and dr and di["samples"] and dr["samples"]:
            if (dr["samples"] - dr["alive"]) + dr["unresolved"] < (di["samples"] - di["alive"]):
                mono_bad += 1
    for s in sorted(sites, key=lambda s: -s["bytes"])[:30]:
        cells = []
        first = "-"
        worst = 0.0
        for b in B:
            d = s["b"][b]
            n = d["samples"]
            if n == 0:
                cells.append("n/a" if d["unresolved"] == 0 else f"unres {int(d['unresolved'])}")
                continue
            dead = n - d["alive"]
            lo, hi = wilson(dead, n)
            worst = max(worst, hi - lo)
            flag = "*" if n < 20 else ""
            cells.append(f"{pct(dead, n)}{flag}")
            if first == "-" and dead / n >= 0.5:
                first = b
        hold = []
        for b in ("iteration", "return"):
            d = s["b"][b]
            best = max(HOLDERS, key=lambda k: d[k])
            hold.append(best if d[best] else "-")
        k = s["key"]
        md.append(f"| {k[1]} {k[2]} {k[3]} {k[4]} | {s['side']} | {s['births']:,} | {s['bytes']:,} | " + " | ".join(cells)
                  + f" | {first} | ±{50 * worst:.1f} pts | {'/'.join(hold)} |")
    md.append("")
    md.append(f"Monotone check (dead at return >= dead at iteration, per site): {mono_bad} sites violate. "
              "`*`: fewer than 20 decided samples.")
    return md, sh, births, bytes_, arena


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument("--prog", nargs=4, action="append", metavar=("NAME", "SITES", "CENSUS", "MEASURE"), required=True)
    ap.add_argument("--out", type=Path, required=True)
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    allsites = []
    summary = ["# LIFE I1 combined", "", "| program | births | bytes | covered of arena | " +
               " | ".join(f"dead@{b[:4]} births / bytes (cover)" for b in B) + " |", "|---|---|---|---|" + "---|" * len(B)]
    arena_all = 0
    for name, sites_f, census, measure in a.prog:
        sites, totals = load(sites_f, census, measure)
        md, sh, births, bytes_, arena = report(name, sites, totals)
        arena_all += arena
        (a.out / f"{name}.md").write_text("\n".join(md) + "\n")
        cells = [f"{pct(sh[b][1], sh[b][0])} / {pct(sh[b][3], sh[b][2])} ({pct(sh[b][0], births)})" for b in B]
        summary.append(f"| {name} | {births:,} | {bytes_:,} | {pct(bytes_, arena)} | " + " | ".join(cells) + " |")
        for s in sites:
            s["kind"] = s["kind"]
        allsites += sites
    sh = shares(allsites)
    births = sum(s["births"] for s in allsites)
    bytes_ = sum(s["bytes"] for s in allsites)
    cells = [f"{pct(sh[b][1], sh[b][0])} / {pct(sh[b][3], sh[b][2])} ({pct(sh[b][0], births)})" for b in B]
    summary.append(f"| **combined** | {births:,} | {bytes_:,} | {pct(bytes_, arena_all)} | " + " | ".join(cells) + " |")
    summary.append("")
    summary.append("## Combined by kind")
    by_kind = defaultdict(list)
    for s in allsites:
        by_kind[s["kind"]].append(s)
    summary += table(sorted(by_kind.items()), births, bytes_)
    summary.append("")
    summary.append("## Combined holders of survivors")
    for b in B:
        summary.append(f"- {b}: {holders(allsites, b)}")
    (a.out / "SUMMARY.md").write_text("\n".join(summary) + "\n")
    print("\n".join(summary))


if __name__ == "__main__":
    main()
