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
the perry_life_ entry points.
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
