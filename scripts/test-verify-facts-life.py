#!/usr/bin/env python3
"""LIFE G1 controls: lifetime claims under PERRY_VERIFY_FACTS=1.

Every fixture compiled in check mode (sampling N=1) must print node's output
with rc 0; a fixture with census claims must execute deaths, and one whose
statement list is left only by `break` / `continue` must carry exit death
points (death `exit` in the binary's claim records). Each `fate:`
sabotage plants a Proven Dropped fate on a parameter the callee keeps: the run
must exit 86 with one JSON line naming the expected holder. The `fatebet:`
sabotage must print one PERRY_BET_FAILED line, rc 0 and node's output.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

# (fixture, needs deaths, needs exit death points) -- the correct arm.
FIXTURES = [
    ("c1_global", False, False),
    ("c3_other_param", False, False),
    ("c5_pass_on", False, False),
    ("c8_recursion", False, False),
    ("c9_dropped", True, False),
    ("c10_bet", False, False),
    ("c11_returned", False, False),
    ("c12_exit", True, True),
]
EXIT_DEATH = b'"death":{"at":"exit"'
# (fixture, sabotage, holder phase the report must name), written before any run.
SABOTAGES = [
    ("c1_global", "fate:keep#0", ("global_root",)),
    ("c3_other_param", "fate:put#1", ("worklist_drain", "remembered_set", "global_root")),
    ("c5_pass_on", "fate:outer#0", ("worklist_drain", "remembered_set", "global_root")),
    ("c11_returned", "fate:id#0", ("native_stack", "shadow_stack")),
]
BET = ("c10_bet", "fatebet:keep#0")


def run(args, **kw):
    return subprocess.run(args, timeout=600, capture_output=True, **kw)


def totals(stderr):
    for line in stderr.decode(errors="replace").splitlines():
        if "PERRY_LIFE_TOTALS" in line:
            return json.loads(line)
    return None


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    perry = args.perry.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    fixtures = Path(__file__).with_name("verify_facts_life").resolve()
    env = dict(os.environ, PERRY_VERIFY_FACTS="1", PERRY_VERIFY_FACTS_LIFE_EVERY="1",
               PERRY_VERIFY_FACTS_LIFE_REPORT="1", PERRY_NO_CACHE="1",
               PERRY_DISABLE_BUILD_CACHE="1", PERRY_NO_LINK_CACHE="1",
               PERRY_NO_ARCHIVE_CACHE="1", PERRY_ALLOW_PERRY_FEATURES="1")
    env.pop("PERRY_FACTS_SABOTAGE", None)
    failures = []

    def build(name, sabotage=None):
        e = dict(env)
        if sabotage:
            e["PERRY_FACTS_SABOTAGE"] = sabotage
        exe = out / (name + ("." + sabotage.replace(":", "_").replace("#", "_") if sabotage else ""))
        c = run([str(perry), "compile", "--cache-dir", str(out / "cache"),
                 str(fixtures / (name + ".ts")), "-o", str(exe)], env=e)
        (out / (exe.name + ".compile")).write_bytes(c.stdout + c.stderr)
        assert c.returncode == 0, (name, sabotage, c.stderr[-2000:])
        return run([str(exe)], env=e), exe

    for name, needs_deaths, needs_exit in FIXTURES:
        want = (fixtures / (name + ".node.txt")).read_bytes()
        r, exe = build(name)
        t = totals(r.stderr)
        exits = exe.read_bytes().count(EXIT_DEATH)
        ok = (r.returncode == 0 and r.stdout == want
              and (not needs_deaths or (t and t["deaths_executed"] > 0))
              and (not needs_exit or exits > 0))
        print(f"fixture {name}: rc={r.returncode} out_eq={r.stdout == want} exit_sites={exits} totals={t} "
              f"{'OK' if ok else 'FAIL'}")
        if not ok:
            failures.append(name)
    for name, sabotage, holders in SABOTAGES:
        r, _ = build(name, sabotage)
        lines = [l for l in r.stderr.decode(errors="replace").splitlines() if "PERRY_FALSE_PROOF" in l]
        phase = None
        if len(lines) == 1:
            value = json.loads(lines[0])
            phase = (value.get("holder") or {}).get("phase")
            ok = (r.returncode == 86 and value.get("verdict") == "Lifetime"
                  and any(h in (phase or "") for h in holders))
        else:
            ok = False
        print(f"sabotage {name} {sabotage}: rc={r.returncode} lines={len(lines)} holder={phase} "
              f"want={holders} {'OK' if ok else 'FAIL'}")
        if not ok:
            failures.append(name + ":" + sabotage)
            (out / (name + ".sabotage.stderr")).write_bytes(r.stderr)
    name, sabotage = BET
    want = (fixtures / (name + ".node.txt")).read_bytes()
    r, _ = build(name, sabotage)
    bets = [l for l in r.stderr.decode(errors="replace").splitlines() if "PERRY_BET_FAILED" in l]
    ok = r.returncode == 0 and r.stdout == want and len(bets) == 1
    print(f"bet {name} {sabotage}: rc={r.returncode} out_eq={r.stdout == want} bet_lines={len(bets)} "
          f"{'OK' if ok else 'FAIL'}")
    if not ok:
        failures.append(name + ":" + sabotage)
    print("FAILURES:", failures if failures else "none")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
