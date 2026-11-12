#!/usr/bin/env python3
"""LIFE I1 controls: the lifetime measurement (PERRY_LIFE_MEASURE).

Each fixture is compiled with the measure knob (an instrument knob: it links
the instrumented runtime variant) and run with PERRY_LIFE_MEASURE_RATE=1
(every birth sampled). It must print node's output with rc 0, and its
measured site (function, kind) must give the answer written below before any
run. The negative control runs m2 with PERRY_FACTS_SABOTAGE=lifekeep (every
sample also stored in a global root): m2 must then report 0% dead, so its
check must FAIL. m1 must never report a death. The default archives offered
under the instrumented names must make a measure compile fail to link on
the perry_life_ entry points. The allocation census (PERRY_LIFE_ALLOC) at
one sample per byte must give sampled covered / uncovered bytes EQUAL to its
exact counts on a1_alloc_mix (a known mix of claimed object literals and
unclaimed runtime strings).
"""
import argparse
import os
import shutil
import subprocess
from pathlib import Path

# fixture -> (function, kind, [(boundary, test, value)]); dead = samples - alive.
EXPECT = {
    "m1_global": ("fill", "object", [("iteration", "alive_share", 1.0), ("iteration", "min_samples", 200),
                                     ("any", "no_dead", 0), ("turn", "alive_share", 1.0)]),
    "m2_body": ("sum", "object", [("iteration", "min_samples", 200), ("iteration", "dead_share", 1.0)]),
    "m3_carried": ("run", "object", [("iteration", "min_samples", 200), ("iteration", "alive_at_least", 0.95),
                                     ("return", "dead_at_least", 0.95)]),
    "m4_returned": ("make", "object", [("return", "min_samples", 200), ("return", "alive_share", 1.0),
                                       ("caller", "dead_at_least", 0.95)]),
    "m5_arg_field": ("run", "object", [("iteration", "min_samples", 200), ("iteration", "dead_at_least", 0.95)]),
    "m6_timer": ("arm", "object", [("turn", "min_samples", 20), ("turn", "alive_share", 1.0)]),
    "m7_throw": ("inner", "object", [("return", "min_samples", 100), ("return", "dead_share", 1.0),
                                     ("return", "all_late", 0)]),
    "m8_callee_dyn": ("make", "object", [("dyn", "min_samples", 200), ("dyn", "dead_share", 1.0)]),
    # The return probe's minor promotes about half of add's survivors (old at the dynamic boundary: unresolved,
    # never guessed); the young half decides it.
    "m9_callee_acc": ("add", "object", [("dyn", "min_samples", 90), ("dyn", "alive_share", 1.0)]),
}


def run(args, **kw):
    return subprocess.run(args, timeout=900, capture_output=True, **kw)


def load(sites, tsv):
    keys = {}
    for line in sites.read_text().splitlines():
        sid, key = line.split("\t", 1)
        keys[sid] = key.split("\t")
    rows = []
    lines = tsv.read_text().splitlines()
    head = lines[0].split("\t")
    for line in lines[1:]:
        if line.startswith("#"):
            continue
        r = dict(zip(head, line.split("\t")))
        k = keys.get(r["site"], ["?"] * 5)
        r.update(module=k[0], function=k[1], kind=k[2], cls=k[3], offset=k[4])
        rows.append(r)
    return rows


def check(rows, function, kind, tests):
    """Returns the failed tests (empty: the answer is right)."""
    mine = [r for r in rows if r["function"] == function and r["kind"] == kind]
    if not mine:
        return ["no site"]
    agg = {}
    for r in mine:
        a = agg.setdefault(r["boundary"], {"samples": 0, "alive": 0, "late": 0})
        for f in a:
            a[f] += int(r[f])
    bad = []
    for b, test, v in tests:
        if test == "no_dead":
            if any(a["samples"] - a["alive"] for a in agg.values()):
                bad.append("dead reported")
            continue
        a = agg.get(b, {"samples": 0, "alive": 0, "late": 0})
        n, alive = a["samples"], a["alive"]
        dead = n - alive
        ok = {
            "min_samples": n >= v,
            "alive_share": n > 0 and alive / n >= v,
            "dead_share": n > 0 and dead / n >= v,
            "alive_at_least": n > 0 and alive / n >= v,
            "dead_at_least": n > 0 and dead / n >= v,
            "all_late": n > 0 and a["late"] == n,
        }[test]
        if not ok:
            bad.append(f"{b}:{test}:{v} (samples {n} alive {alive} late {a['late']})")
    return bad


def check_alloc(path):
    """The alloc TSV at one sample per byte: sampled bytes per state equal the exact ones."""
    meta, types, sampled = {}, {}, {"covered": 0, "uncovered": 0, "unplaced": 0}
    for line in path.read_text().splitlines():
        f = line.split("\t")
        if f[0] == "#meta":
            meta = dict(kv.split("=", 1) for kv in f[1:])
        elif f[0] == "#type":
            types[f[1]] = [int(x) for x in f[2:8]]
        elif f[0] == "sample":
            sampled[f[1]] += int(f[4])
    bad = []
    if meta.get("interval") != "1":
        bad.append("interval " + meta.get("interval", "?"))
    total = sum(v[1] + v[3] for v in types.values())
    covered = sum(v[5] for v in types.values())
    if sampled["covered"] != covered:
        bad.append(f"covered sampled {sampled['covered']} != exact {covered}")
    if sampled["uncovered"] + sampled["unplaced"] != total - covered:
        bad.append(f"uncovered sampled {sampled['uncovered']}+{sampled['unplaced']} != exact {total - covered}")
    obj, st = types.get("object", [0] * 6), types.get("string", [0] * 6)
    if obj[4] < 300:
        bad.append(f"covered objects {obj[4]} < 300")
    if st[1] + st[3] < 300 * 100 or st[5] != 0:
        bad.append(f"strings {st[1] + st[3]} B (want >= 30000), covered {st[5]} (want 0)")
    for k in ("claims_outside", "walk_breaks", "inline_slow_bad"):
        if meta.get(k) != "0":
            bad.append(f"{k}={meta.get(k)}")
    return bad


# LIFE I3 fixture -> [(kind, function, loop ordinal or None, {column: exact value}, {column: minimum})].
REGIONS = {
    "r1_clean_loop": [("loop", "run", "#0", {"sampled": 200, "clean": 200, "dirty": 0, "unresolved": 0, "empty": 0},
                       {"bytes": 1})],
    "r2_push_outer": [("loop", "fill", "#0", {"sampled": 200, "dirty": 200, "surv1": 200, "surv_objs": 200,
                                              "unresolved": 0, "held_by_object": 200}, {})],
    "r3_every10": [("loop", "run", "#0", {"sampled": 200, "clean": 180, "dirty": 20, "surv1": 20, "unresolved": 0}, {})],
    "r4_fn_clean": [("body", "work", None, {"sampled": 20, "clean": 20, "unresolved": 0}, {}),
                    ("loop", "work", "#0", {"sampled": 1000, "dirty": 1000}, {})],
    "r5_runtime_garbage": [("loop", "run", "#0", {"sampled": 200, "clean": 200, "unresolved": 0}, {"bytes": 20000})],
    # LIFE I3d: promotion inside the iteration; the verdict must not depend on the sampling density
    # (DENSITY below re-runs them sparse). r8: only shape key lists (engine metadata) outlive an iteration.
    "r6_promote_clean": [("loop", "run", "#0", {"sampled": 40, "clean": 40, "dirty": 0, "unresolved": 0}, {})],
    "r7_promote_keep1": [("loop", "run", "#0", {"sampled": 40, "dirty": 40, "surv1": 40, "surv_objs": 40,
                                                "unresolved": 0}, {})],
    "r8_shape_remint": [("loop", "run", "#0", {"sampled": 40, "clean": 40, "dirty": 0, "unresolved": 0}, {})],
}


def check_regions(sites, tsv, tests):
    """The region rows of the fixture's regions against the known answer; [] when right."""
    keys = {}
    for line in sites.read_text().splitlines():
        sid, key = line.split("\t", 1)
        keys[sid] = key.split("\t")
    lines = tsv.read_text().splitlines()
    head = lines[0].split("\t")
    rows = [dict(zip(head, l.split("\t"))) for l in lines[1:] if not l.startswith("#")]
    bad = []
    for kind, function, ordinal, exact, least in tests:
        mine = [r for r in rows if r["kind"] == kind and keys.get(r["id"], ["?"])[0] == kind
                and keys[r["id"]][2] == function and (ordinal is None or keys[r["id"]][3] == ordinal)]
        if len(mine) != 1:
            bad.append(f"{kind} {function} {ordinal}: {len(mine)} rows")
            continue
        r = {k: float(v) for k, v in mine[0].items() if k not in ("kind", "id")}
        r["held_by_object"] = r["h_object"] + r["h_remembered"]
        for k, v in exact.items():
            if r[k] != v:
                bad.append(f"{kind} {function} {ordinal or ''} {k}={r[k]:.0f} want {v}")
        for k, v in least.items():
            if r[k] < v:
                bad.append(f"{kind} {function} {ordinal or ''} {k}={r[k]:.0f} want >= {v}")
    # Hook coverage: every body is named (a copy compiled under another address included), and every
    # region the run saw has a site row.
    unreg = [" ".join(k) for k in keys.values() if k[0] in ("loop", "body") and k[2].startswith("<unregistered")]
    if unreg:
        bad.append(f"unregistered bodies: {unreg}")
    unknown = [r["id"] for r in rows if r["kind"] != "turn" and r["id"] not in keys]
    if unknown:
        bad.append(f"region ids without a site row: {len(unknown)}")
    restarted = sum(float(r.get("restarted", 0)) for r in rows)
    if restarted:
        print(f"  note: {restarted:.0f} iterations ended by the next start (no back-edge hook)")
    return bad


DENSITY = ("r6_promote_clean", "r7_promote_keep1", "r8_shape_remint")


def density(name, exe, sites, tests, go, dense_out):
    """The same fixture decided sparsely (`PERRY_LIFE_REGIONS_SHIFT=3`: about one instance in eight): every sampled
    instance gets the dense verdict (all clean, or all dirty with the dense survivor count per instance);
    r8 also runs the `lifemeta` control, which counts shape key lists as JS values and must fail."""
    failures = []
    kind, function, ordinal, exact, _ = tests[0]
    r, tsv = go(name, exe, {"PERRY_LIFE_REGIONS_RATE": "0", "PERRY_LIFE_REGIONS_SHIFT": "3"})
    rows = region_rows(sites, tsv, kind, function, ordinal) if tsv.exists() else []
    bad = []
    if r.returncode != 0 or r.stdout != dense_out or len(rows) != 1:
        bad.append(f"rc={r.returncode} rows={len(rows)}")
    else:
        x = rows[0]
        n = x["sampled"]
        if not 0 < n < exact["sampled"]:
            bad.append(f"sparse sampled {n:.0f} (want 0 < n < {exact['sampled']})")
        if x["unresolved"]:
            bad.append(f"unresolved {x['unresolved']:.0f}")
        if name == "r8_shape_remint" and not x.get("meta_objs"):
            bad.append("meta_objs 0: no key list minted, the fixture does not exercise the exclusion")
        if exact.get("clean") == exact["sampled"] and x["clean"] != n:
            bad.append(f"clean {x['clean']:.0f} of {n:.0f}")
        if exact.get("dirty") == exact["sampled"] and (x["dirty"] != n or x["surv1"] != n or x["surv_objs"] != n):
            bad.append(f"dirty {x['dirty']:.0f} surv1 {x['surv1']:.0f} objs {x['surv_objs']:.0f} of {n:.0f}")
    print(f"{name} sparse: sampled {rows[0]['sampled'] if rows else '-'} "
          f"clean {rows[0]['clean'] if rows else '-'} dirty {rows[0]['dirty'] if rows else '-'}: "
          f"{'dense == sparse' if not bad else 'DIFFERS ' + '; '.join(bad)}")
    if bad:
        failures.append(name + ":density")
    if name == "r8_shape_remint":
        r2, tsv2 = go(name, exe, {"PERRY_FACTS_SABOTAGE": "lifemeta"})
        rows2 = region_rows(sites, tsv2, kind, function, ordinal) if tsv2.exists() else []
        meta = rows2[0]["dirty"] if rows2 else 0
        ok = r2.returncode == 0 and meta > 0
        print(f"control lifemeta {name}: dirty {meta:.0f} -> {'FAILS (good)' if ok else 'passes (control BROKEN)'}")
        if not ok:
            failures.append("control:lifemeta:regions")
    return failures


def region_rows(sites, tsv, kind, function, ordinal):
    keys = {}
    for line in sites.read_text().splitlines():
        sid, key = line.split("\t", 1)
        keys[sid] = key.split("\t")
    lines = tsv.read_text().splitlines()
    head = lines[0].split("\t")
    rows = [dict(zip(head, l.split("\t"))) for l in lines[1:] if not l.startswith("#")]
    return [{k: float(v) for k, v in x.items() if k not in ("kind", "id")} for x in rows
            if x["kind"] == kind and keys.get(x["id"], ["?"])[0] == kind and keys[x["id"]][2] == function
            and (ordinal is None or keys[x["id"]][3] == ordinal)]


def regions(build, out, fixtures, env):
    failures = []

    def go(name, exe, extra=None):
        tsv = out / (name + (".keep" if extra else "") + ".regions.tsv")
        tsv.unlink(missing_ok=True)
        e = dict(env, PERRY_LIFE_REGIONS=str(tsv), PERRY_LIFE_REGIONS_RATE="1")
        e.update(extra or {})
        r = run([str(exe)], env=e)
        return r, tsv

    for name, tests in REGIONS.items():
        c, exe, sites = build(name)
        if c.returncode != 0:
            print(f"{name}: compile rc={c.returncode} FAIL")
            (out / (name + ".compile.txt")).write_bytes(c.stdout + c.stderr)
            failures.append(name + ":compile")
            continue
        alloc = out / (name + ".alloc.tsv")
        r, tsv = go(name, exe, None)
        same = r.stdout == (fixtures / (name + ".node.txt")).read_bytes()
        bad = check_regions(sites, tsv, tests) if tsv.exists() else ["no tsv"]
        if name == "r5_runtime_garbage":
            # Runtime-only garbage: no census birth site in run.
            births = [k for k in (l.split("\t")[1:] for l in sites.read_text().splitlines())
                      if k[0] not in ("loop", "body") and len(k) > 1 and k[1] == "run"]
            if births:
                bad.append(f"census births in run: {len(births)}")
        if name == "r1_clean_loop" and tsv.exists():
            # Consistency: bytes born before the first turn + in every turn + after the last = the census's
            # total less its unplaced bytes (exact), from a second run that writes the census file too.
            r3, tsv3 = go(name, exe, {"PERRY_LIFE_ALLOC": str(alloc)})
            t = dict(kv.split("=", 1) for kv in tsv3.read_text().splitlines()[-1].split("\t")[1:])
            tot = unplaced = 0
            for line in alloc.read_text().splitlines():
                f = line.split("\t")
                if f[0] == "#type":
                    tot += int(f[3]) + int(f[5])
                elif f[0] == "#meta":
                    unplaced = int(dict(kv.split("=", 1) for kv in f[1:]).get("unplaced_bytes", "0"))
            mine = int(t["before_bytes"]) + int(t["turn_bytes"]) + int(t["tail_bytes"])
            print(f"{name}: regions before+turns+tail {mine} B, census total-unplaced {tot - unplaced} B, "
                  f"exit={t['exit']} unobserved={t['unobserved']} busy={t['busy']}")
            if mine != tot - unplaced or t["exit"] != "1" or t["busy"] != "0":
                bad.append("consistency")
        ok = r.returncode == 0 and same and not bad
        print(f"{name}: rc={r.returncode} node={'same' if same else 'DIFF'} regions: "
              f"{'OK' if not bad else 'FAIL ' + '; '.join(bad)}")
        if not ok:
            failures.append(name)
            (out / (name + ".stderr")).write_bytes(r.stderr)
        if name in DENSITY:
            failures += density(name, exe, sites, tests, go, r.stdout)
        if name == "r1_clean_loop":
            r2, tsv2 = go(name, exe, {"PERRY_FACTS_SABOTAGE": "lifekeep"})
            bad2 = check_regions(sites, tsv2, tests) if tsv2.exists() else []
            ok2 = r2.returncode == 0 and bool(bad2)
            print(f"control lifekeep {name} regions: rc={r2.returncode} check "
                  f"{'FAILS (' + bad2[0] + ')' if bad2 else 'passes'}: {'OK' if ok2 else 'FAIL'}")
            if not ok2:
                failures.append("control:lifekeep:regions")
    return failures


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    perry = args.perry.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    fixtures = Path(__file__).with_name("life_measure").resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith("PERRY_")}
    failures = []

    def build(name, e=None):
        sites = out / (name + ".sites")
        sites.unlink(missing_ok=True)
        exe = out / name
        c = run([str(perry), "compile", "--cache-dir", str(out / "cache"), str(fixtures / (name + ".ts")),
                 "-o", str(exe)], env=dict(e or env, PERRY_LIFE_MEASURE=str(sites), PERRY_NO_CACHE="1"))
        return c, exe, sites

    def measure(name, exe, extra=None):
        tsv = out / (name + (".keep" if extra else "") + ".tsv")
        tsv.unlink(missing_ok=True)
        r = run([str(exe)], env=dict(env, PERRY_LIFE_MEASURE=str(tsv), PERRY_LIFE_MEASURE_RATE="1", **(extra or {})))
        return r, tsv

    for name, (function, kind, tests) in EXPECT.items():
        c, exe, sites = build(name)
        if c.returncode != 0:
            print(f"{name}: compile rc={c.returncode} FAIL")
            (out / (name + ".compile.txt")).write_bytes(c.stdout + c.stderr)
            failures.append(name + ":compile")
            continue
        r, tsv = measure(name, exe)
        want = (fixtures / (name + ".node.txt")).read_bytes()
        same = r.stdout == want
        bad = check(load(sites, tsv), function, kind, tests) if tsv.exists() else ["no tsv"]
        ok = r.returncode == 0 and same and not bad
        print(f"{name}: rc={r.returncode} node={'same' if same else 'DIFF'} {function}/{kind}: "
              f"{'OK' if not bad else 'FAIL ' + '; '.join(bad)}")
        if not ok:
            failures.append(name)
            (out / (name + ".stderr")).write_bytes(r.stderr)
        if name in ("m2_body", "m8_callee_dyn"):
            r2, tsv2 = measure(name, exe, {"PERRY_FACTS_SABOTAGE": "lifekeep"})
            bad2 = check(load(sites, tsv2), function, kind, tests) if tsv2.exists() else ["no tsv"]
            dead0 = not check(load(sites, tsv2), function, kind, [("any", "no_dead", 0)]) if tsv2.exists() else False
            ok2 = r2.returncode == 0 and bool(bad2) and dead0
            print(f"control lifekeep {name}: rc={r2.returncode} check {'FAILS' if bad2 else 'passes'}, "
                  f"0% dead={dead0}: {'OK' if ok2 else 'FAIL'}")
            if not ok2:
                failures.append("control:lifekeep:" + name)
    # LIFE I2 A: the allocation census at one sample per byte. The sampled
    # covered / uncovered bytes are the exact ones (one predicate: claimed by
    # a census birth while in the epoch map), on a known mix: 300 object
    # literals at a census site, 300 runtime strings of 100..106 characters.
    name = "a1_alloc_mix"
    c, exe, sites = build(name)
    alloc = out / (name + ".alloc.tsv")
    alloc.unlink(missing_ok=True)
    bad = []
    if c.returncode != 0:
        bad.append(f"compile rc={c.returncode}")
        (out / (name + ".compile.txt")).write_bytes(c.stdout + c.stderr)
    else:
        r, _ = measure(name, exe, {"PERRY_LIFE_ALLOC": str(alloc), "PERRY_LIFE_ALLOC_SAMPLE": "1"})
        if r.returncode != 0 or r.stdout != (fixtures / (name + ".node.txt")).read_bytes():
            bad.append(f"rc={r.returncode} or output differs from node")
        if not alloc.exists():
            bad.append("no alloc tsv")
        else:
            bad += check_alloc(alloc)
    print(f"{name}: sampled = exact at 1 B: {'OK' if not bad else 'FAIL ' + '; '.join(bad)}")
    if bad:
        failures.append(name)
    # LIFE I3: the region ceiling (PERRY_LIFE_REGIONS, every instance sampled).
    failures += regions(build, out, fixtures, env)
    # The default archives under the instrumented names: no perry_life_ hooks.
    plain = out / "default-as-instr"
    plain.mkdir(exist_ok=True)
    for stem in ("libperry_runtime", "libperry_stdlib"):
        for suffix in (".a", ".a.gc-effects"):
            src = perry.parent / (stem + suffix)
            if src.exists():
                shutil.copyfile(src, plain / (stem + "_instr" + suffix))
    c, _, _ = build("m2_body", dict(env, PERRY_RUNTIME_DIR=str(plain)))
    ok = c.returncode != 0 and b"perry_life_" in c.stdout + c.stderr
    print(f"no-hooks m2_body on the default runtime: compile rc={c.returncode} {'OK' if ok else 'FAIL'}")
    if not ok:
        failures.append("no-hooks")
        (out / "no-hooks.txt").write_bytes(c.stdout + c.stderr)
    print("FAILURES: " + (", ".join(failures) if failures else "none"))
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
