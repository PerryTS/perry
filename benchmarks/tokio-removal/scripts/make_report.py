#!/usr/bin/env python3
"""Render benchmarks/tokio-removal/results/*.json[l] into results/summary.md + summary.json.
usage: make_report.py <results-dir>"""
import json, os, statistics as st, sys, collections
R = sys.argv[1]
def jl(n):
    p = os.path.join(R, n)
    return [json.loads(l) for l in open(p) if l.strip().startswith("{")] if os.path.exists(p) else []
def js(n):
    p = os.path.join(R, n); return json.load(open(p)) if os.path.exists(p) else None
def pct(b, a):
    return "–" if not b or a is None else f"{100*(a-b)/b:+.1f}%"
def med(xs): xs = [x for x in xs if x is not None]; return st.median(xs) if xs else None
def spread(xs):
    xs = [x for x in xs if x is not None]; return (min(xs), max(xs)) if xs else (None, None)
def fmt(x, d=0):
    if x is None: return "–"
    return f"{x:,.{d}f}"
def mib(x): return "–" if x is None else f"{x/1048576:.2f}"
out = []; summ = {}
w = out.append

# ---------- binary sizes ----------
auto = {(r["arm"], r["probe"]): r for r in jl("linux-auto.jsonl")}
pb = {(r["arm"], r["probe"], r["mode"]): r for r in jl("linux-prebuilt.jsonl")}
probes = [p for p in ["hello","fetch_local","http","https","net","tls_net","ws","crypto","zlib","child","timers","worker","backend","container","bench_http_server","bench_fetch_client"]]
w("## Binary size (Linux x86_64, release profile)\n")
w("Auto-optimize ON (the user path in a source checkout) = stripped output; `+sym` = `PERRY_KEEP_SYMBOLS=1`. "
  "Prebuilt = out-of-tree install linking the prebuilt full `libperry_stdlib.a` (+ ext archives); "
  "`PERRY_NO_AUTO_OPTIMIZE=1` gives byte-identical output to it in every row (see raw data).\n")
w("| probe | auto before | auto after | Δ | +sym before | +sym after | Δ | prebuilt before | prebuilt after | Δ |")
w("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
summ["sizes"] = {}
for p in probes:
    b, a = auto.get(("before", p), {}), auto.get(("after", p), {})
    pbb, pba = pb.get(("before", p, "prebuilt-noauto"), {}), pb.get(("after", p, "prebuilt-noauto"), {})
    row = (b.get("size"), a.get("size"), b.get("size_sym"), a.get("size_sym"), pbb.get("size"), pba.get("size"))
    summ["sizes"][p] = dict(zip(["auto_before","auto_after","sym_before","sym_after","prebuilt_before","prebuilt_after"], row))
    fb = " †" if p != "hello" and row[0] is not None and row[0] == row[4] else ""
    summ["sizes"][p]["before_fell_back_to_prebuilt"] = bool(fb)
    w(f"| {p}{fb} | {fmt(row[0])} | {fmt(row[1])} | {pct(row[0],row[1])} | {fmt(row[2])} | {fmt(row[3])} | {pct(row[2],row[3])} | {fmt(row[4])} | {fmt(row[5])} | {pct(row[4],row[5])} |")
w("")
w("† BEFORE's auto-optimize cargo build FAILED for this program's feature set (`async-runtime,web-fetch`: `unresolved import base64` in perry-stdlib) and perry fell back to the prebuilt full stdlib, so the BEFORE auto number equals its prebuilt number. Not a like-for-like auto row; `fetch_local` (fetch + a server) is the comparable fetch row. Fixed somewhere in the window — AFTER builds that set.\n")
w("### Subject-is-live check: tokio-family symbols in the `+sym` binaries\n")
w("| probe | tokio before→after | hyper | reqwest | h2 | tower | mio | tokio_rustls | turnloop | `tokio-1.x` strings (stripped) |")
w("|---|---|---|---|---|---|---|---|---|---|")
for p in probes:
    b, a = auto.get(("before", p), {}), auto.get(("after", p), {})
    c = lambda k: f"{b.get(k,'–')}→{a.get(k,'–')}"
    w(f"| {p} | {c('nm_tokio')} | {c('nm_hyper')} | {c('nm_reqwest')} | {c('nm_h2')} | {c('nm_tower')} | {c('nm_mio')} | {c('nm_tokio_rustls')} | {c('nm_turnloop')} | {c('strings_tokio_1x')} |")
w("")
# attribution
att = js("linux-attribution.json")
if att:
    w("### Size attribution by crate (symbol bytes in the `+sym` binary)\n")
    for p in att:
        b, a = att[p].get("before", {}), att[p].get("after", {})
        keys = sorted(set(b) | set(a), key=lambda k: -max(b.get(k, {}).get("bytes", 0), a.get(k, {}).get("bytes", 0)))
        tb = sum(v["bytes"] for v in b.values()); ta = sum(v["bytes"] for v in a.values())
        w(f"**{p}** — attributed symbol bytes {fmt(tb)} → {fmt(ta)} ({pct(tb, ta)})\n")
        w("| crate | before | after | Δ bytes |"); w("|---|---:|---:|---:|")
        for k in keys[:22]:
            x, y = b.get(k, {}).get("bytes", 0), a.get(k, {}).get("bytes", 0)
            w(f"| {k} | {fmt(x)} | {fmt(y)} | {y-x:+,} |")
        w("")
# ---------- archives ----------
arc = js("linux-archives.json")
if arc:
    w("## Archive sizes (Linux, release)\n")
    w("| archive | before | after | Δ |"); w("|---|---:|---:|---:|")
    names = sorted(set(arc["before"]) | set(arc["after"]), key=lambda n: (not n.startswith("libperry_runtime") and not n.startswith("libperry_stdlib"), n))
    for n in names:
        x, y = arc["before"].get(n), arc["after"].get(n)
        w(f"| {n} | {mib(x)} MiB | {mib(y)} MiB | {pct(x,y) if x and y else ('removed' if x else 'new')} |")
    w("")
w("### Per-program auto-optimized archives\n")
w("| probe | before: runtime / stdlib / ext (MiB) | after: runtime / stdlib / ext (MiB) |"); w("|---|---|---|")
def parse_arch(s):
    d = {}
    for kv in (s or "").strip(",").split(","):
        if "=" in kv: k, v = kv.split("="); d[k] = int(v)
    return d
for p in probes:
    ds = []
    for arm in ("before", "after"):
        d = parse_arch(auto.get((arm, p), {}).get("auto_archives"))
        if not d: ds.append("(none: tiny-program path)" if p == "hello" else "(none: auto-opt cargo build failed, prebuilt fallback)"); continue
        ext = sum(v for k, v in d.items() if "ext" in k)
        ds.append(f"{mib(d.get('libperry_runtime.a'))} / {mib(d.get('libperry_stdlib.a'))} / {mib(ext)}")
    w(f"| {p} | {ds[0]} | {ds[1]} |")
w("")
# ---------- compile time ----------
bld = js("linux-build.json")
w("## Compile time\n")
if bld:
    w("`cargo build --release -j4` from a clean target dir, both arms concurrently on perrymaster (shared, load 35–75: wall is indicative only; CPU time is the comparable number).\n")
    w("| step | before wall | after wall | before CPU (user+sys) | after CPU | Δ CPU |"); w("|---|---:|---:|---:|---:|---:|")
    for step in ("core", "ext"):
        b, a = bld["before"][step], bld["after"][step]
        cb, ca = b["user"] + b["sys"], a["user"] + a["sys"]
        w(f"| {step} ({b['what']}) | {fmt(b['wall'])} s | {fmt(a['wall'])} s | {fmt(cb)} s | {fmt(ca)} s | {pct(cb,ca)} |")
    w("")
w("`perry compile` per probe (Linux, auto-optimize ON). Cold = empty object cache and the program's feature set not yet built (cargo rebuilds runtime+stdlib+ext); warm = immediate recompile.\n")
w("| probe | cold rebuilt? b/a | cold CPU before | cold CPU after | Δ | warm wall before | warm wall after | Δ |"); w("|---|---|---:|---:|---:|---:|---:|---:|")
for p in probes:
    b, a = auto.get(("before", p), {}), auto.get(("after", p), {})
    cb = (b.get("cold_user") or 0) + (b.get("cold_sys") or 0) if b else None
    ca = (a.get("cold_user") or 0) + (a.get("cold_sys") or 0) if a else None
    w(f"| {p} | {b.get('rebuilt_auto_libs','–')}/{a.get('rebuilt_auto_libs','–')} | {fmt(cb,1)} s | {fmt(ca,1)} s | {pct(cb,ca)} | {fmt(b.get('warm_wall'),2)} s | {fmt(a.get('warm_wall'),2)} s | {pct(b.get('warm_wall'),a.get('warm_wall'))} |")
w("")
mac = jl("mac-compile.jsonl")
if mac:
    m = {(r["arm"], r["probe"]): r for r in mac}
    w("macOS arm64 (dev MacBook, shared, load varied 6→45 between arms — wall only, indicative): \n")
    w("| probe | cold before | cold after | warm before | warm after | size before | size after | Δ size |"); w("|---|---:|---:|---:|---:|---:|---:|---:|")
    for p in [r["probe"] for r in mac if r["arm"] == "before"]:
        b, a = m.get(("before", p), {}), m.get(("after", p), {})
        cw = lambda r: f"{fmt(r.get('cold_wall'),0)} s" if r.get("rc") == 0 else f"failed (rc {r.get('rc')})"
        ww = lambda r: f"{fmt(r.get('warm_wall'),2)} s" if r.get("rc") == 0 else "–"
        w(f"| {p} | {cw(b)} | {cw(a)} | {ww(b)} | {ww(a)} | {fmt(b.get('size'))} | {fmt(a.get('size'))} | {pct(b.get('size'),a.get('size'))} |")
    w("")
# ---------- runtime (mini) ----------
mini = js("mini.json")
if mini:
    S_all = mini["samples"]
    # Invalid cells, excluded (see README): fetch runs that returned no result (both
    # happened right after oha had hammered a dead backend with ~1.1M refused
    # connects — ephemeral-port/mbuf exhaustion), and every backend c=64 cell (the
    # backend died mid-cell in BOTH arms: IdExhausted handle panic, see README).
    bad = lambda s: (s.get("kind") == "fetch" and not s.get("rps")) or (s.get("kind") == "backend" and s.get("conc") == 64)
    S = [s for s in S_all if not bad(s)]
    summ["excluded_samples"] = sum(1 for s in S_all if bad(s))
    w(f"## Runtime (quiet bench mini: {mini['host']}, M1, macOS; {mini['rounds']} interleaved rounds, {mini['duration']} per load cell, median [min–max])\n")
    def cell(kind, conc, key, arm):
        return [s.get(key) for s in S if s.get("kind") == kind and s.get("conc") == conc and s.get("arm") == arm]
    def line(label, kind, conc, key, scale=1, d=0):
        b = [x * scale for x in cell(kind, conc, key, "before") if x is not None]; a = [x * scale for x in cell(kind, conc, key, "after") if x is not None]
        mb, ma = med(b), med(a); sb, sa = spread(b), spread(a)
        summ.setdefault("runtime", {})[f"{label}"] = {"before": mb, "after": ma}
        w(f"| {label} | {fmt(mb,d)} [{fmt(sb[0],d)}–{fmt(sb[1],d)}] | {fmt(ma,d)} [{fmt(sa[0],d)}–{fmt(sa[1],d)}] | {pct(mb,ma)} |")
    w("| metric | before | after | Δ |"); w("|---|---:|---:|---:|")
    for kind, concs in (("http", (1, 64, 256)), ("backend", (1,))):
        for c in concs:
            line(f"{kind} c={c} req/s", kind, c, "rps")
            line(f"{kind} c={c} p50 ms", kind, c, "p50_ms", d=3)
            line(f"{kind} c={c} p99 ms", kind, c, "p99_ms", d=3)
            line(f"{kind} c={c} CPU µs/req", kind, c, "cpu_us_per_req", d=1)
            line(f"{kind} c={c} RSS peak KiB", kind, c, "rss_peak_kb")
            line(f"{kind} c={c} threads after load", kind, c, "threads_after")
    for kind in ("http_idle", "backend_idle"):
        line(f"{kind} RSS KiB", kind, None, "rss_kb"); line(f"{kind} threads", kind, None, "threads")
    for c in (1, 16):
        line(f"fetch client c={c} req/s", "fetch", c, "rps"); line(f"fetch client c={c} CPU µs/req", "fetch", c, "cpu_us_per_req", d=1)
        line(f"fetch client c={c} max RSS KiB", "fetch", c, "maxrss_bytes", scale=1/1024)
    line("hello startup warm ms", "startup", None, "hello_warm_median_s", scale=1000, d=2)
    line("hello startup cold (fresh copy) ms", "startup", None, "hello_cold_s", scale=1000, d=1)
    line("hello max RSS KiB", "startup", None, "hello_maxrss_bytes", scale=1/1024)
    line("backend startup warm ms", "startup", None, "backend_warm_median_s", scale=1000, d=2)
    line("backend startup cold (fresh copy) ms", "startup", None, "backend_cold_s", scale=1000, d=1)
    line("backend startup max RSS KiB", "startup", None, "backend_maxrss_bytes", scale=1/1024)
    w("")
    dg = js("mini-diag.json")
    if dg:
        w("")
        w("Backend at c=8…64 in a FRESH process per cell (5 s each, 3 rounds; `mini-diag.json`) — the replacement for the invalid c=64 cells:\n")
        w("| conc | before req/s | after req/s | Δ | survived (before/after) |"); w("|---|---:|---:|---:|---|")
        for c in (8, 16, 32, 64):
            rb = [x["ok"] / 5 for x in dg["backend"] if x["arm"] == "before" and x["conc"] == c]
            ra = [x["ok"] / 5 for x in dg["backend"] if x["arm"] == "after" and x["conc"] == c]
            sb = sum(x["alive_after"] for x in dg["backend"] if x["arm"] == "before" and x["conc"] == c)
            sa = sum(x["alive_after"] for x in dg["backend"] if x["arm"] == "after" and x["conc"] == c)
            summ.setdefault("runtime", {})[f"backend fresh c={c} req/s"] = {"before": med(rb), "after": med(ra)}
            w(f"| {c} | {fmt(med(rb))} | {fmt(med(ra))} | {pct(med(rb), med(ra))} | {sb}/{len(rb)} vs {sa}/{len(ra)} |")
        fz = collections.Counter((f["arm"], f["ok"]) for f in dg["fetch"])
        w(f"\nfetch-client reliability re-run (10 rounds × c=1,16 per arm): before {fz[('before', True)]}/20 ok, after {fz[('after', True)]}/20 ok.\n")
        w("Peak thread count while working (ps -M sampled every 100 ms): " + ", ".join(f"{t['kind']} {t['peak_under_load']}" for t in dg["threads"] if t["arm"] == "after") + " (AFTER); BEFORE identical: " + ", ".join(f"{t['kind']} {t['peak_under_load']}" for t in dg["threads"] if t["arm"] == "before") + ".\n")
    w(f"Excluded samples: {summ['excluded_samples']}.\n")
    w(f"Host load: start {mini.get('load_start')}, end {mini.get('load_end')}.\n")
    corr = mini.get("correctness", {})
    if corr:
        w("### Correctness on macOS (output vs Node 26.5.1)\n"); w("| probe | before | after |"); w("|---|---|---|")
        for p, v in sorted(corr.items()):
            f = lambda a: ("match" if v[a]["matches_node"] else f"DIFF (rc={v[a]['rc']})")
            w(f"| {p} | {f('before')} | {f('after')} |")
        w("")
# ---------- instructions ----------
ins = jl("linux-instr.jsonl")
if ins:
    w("## Retired instructions (perrymaster, `perf stat -e instructions:u`, two-N differential)\n")
    w("per-op = (I(N2) − I(N1)) / (N2 − N1), medians over reps; fixed = I(N1) − N1·per-op. `ops_loop` is the bare-loop control (pure JS, no event loop): it should not move.\n")
    w("| probe | per-op before | per-op after | Δ | fixed before | fixed after | Δ |"); w("|---|---:|---:|---:|---:|---:|---:|")
    g = collections.defaultdict(list)
    for r in ins:
        if r.get("instructions") is not None: g[(r["arm"], r["probe"], r["n"])].append(r["instructions"])
    ns = sorted({r["n"] for r in ins}); n1, n2 = ns[0], ns[-1]
    summ["instructions"] = {}
    for p in sorted({r["probe"] for r in ins}):
        vals = {}
        for arm in ("before", "after"):
            i1 = med(g.get((arm, p, n1), [])); i2 = med(g.get((arm, p, n2), []))
            if p == "hello": vals[arm] = (None, i1)
            elif i1 is not None and i2 is not None:
                per = (i2 - i1) / (n2 - n1); vals[arm] = (per, i1 - n1 * per)
            else: vals[arm] = (None, None)
        (pb_, fb), (pa, fa) = vals["before"], vals["after"]
        summ["instructions"][p] = {"per_op_before": pb_, "per_op_after": pa, "fixed_before": fb, "fixed_after": fa}
        note = " ‡" if p == "ops_http" else ""
        w(f"| {p}{note} | {fmt(pb_)} | {fmt(pa)} | {pct(pb_,pa)} | {fmt(fb)} | {fmt(fa)} | {pct(fb,fa)} |")
    w("")
    w("‡ `ops_http` is NOT linear in N in either arm (I(100)/I(500)/I(1000)/I(2000) = 104/736/2,280/7,891 M before, 113/1,002/3,347 M after), so its per-op and fixed columns are not meaningful. AFTER additionally holds one socket fd per completed `http.get` round trip (301 open fds after 300 requests vs 6 before) and dies with `ECONNREFUSED` once it reaches the 1024-fd ulimit (~1,015 sequential requests); BEFORE completes N=2000. See README, *Regressions*.\n")
deps = js("linux-deps.json")
if deps:
    w("## Dependency footprint\n")
    b, a = deps["before"], deps["after"]
    w(f"Cargo.lock packages: **{b['lock_packages']} → {a['lock_packages']}** ({pct(b['lock_packages'], a['lock_packages'])}).\n")
    w("| crate | reachable deps before | after | Δ | async/HTTP-stack crates before | after |"); w("|---|---:|---:|---:|---|---|")
    for c in b["crates"]:
        x, y = b["crates"][c], a["crates"].get(c, {})
        w(f"| {c} | {x.get('deps_including_self')} | {y.get('deps_including_self')} | {pct(x.get('deps_including_self'), y.get('deps_including_self'))} | {', '.join(x.get('async_http_stack', [])) or '–'} | {', '.join(y.get('async_http_stack', [])) or '–'} |")
    w("")
    fam_b, fam_a = b["family"], a["family"]
    gone = sorted(set(fam_b) - set(fam_a))
    w(f"Removed from the lockfile: {', '.join(gone)}.\n")
open(os.path.join(R, "summary.md"), "w").write("\n".join(out) + "\n")
json.dump(summ, open(os.path.join(R, "summary.json"), "w"), indent=1)
print("\n".join(out))
