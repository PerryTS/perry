#!/usr/bin/env python3
"""Regression guard for the print-and-diff node-suite.

Runs `scripts/node_suite_run.py` (pre-warm + fast/slow lanes) and compares the
per-module pass counts against a committed floor baseline. FAILS (exit 1) if any
baselined module drops below its floor. Improvements are always accepted and are
reported as `+N` so the baseline can be ratcheted up over time.

This exists because the node-suite is NOT part of the per-PR CI gate (the parity
job is opt-in and runs node 22, while the real oracle is node 26), so a module
can silently regress and still merge green — which is exactly how node:dns once
went 83% -> 0% behind a green build. Run this in the node-26 environment (the box)
on a schedule, or before cutting a release.

Usage:
  node_suite_regression_check.py <perry-bin> <repo-root> [baseline.json]

Exit codes: 0 = no regressions (improvements ok), 1 = at least one regression,
            2 = harness error (could not run / parse).
"""
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_DEFAULT = os.path.dirname(HERE)


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    perry, root = sys.argv[1], sys.argv[2]
    baseline_path = sys.argv[3] if len(sys.argv) > 3 else os.path.join(
        root, "test-parity", "node_suite_baseline.json")

    if not os.path.exists(baseline_path):
        print(f"ERROR: baseline not found: {baseline_path}", file=sys.stderr)
        return 2
    baseline = json.load(open(baseline_path)).get("modules", {})

    runner = os.path.join(root, "scripts", "node_suite_run.py")
    proc = subprocess.run([sys.executable, runner, perry, root],
                          capture_output=True, text=True)
    sys.stderr.write(proc.stderr)
    print(proc.stdout)
    # Fail closed: any non-zero runner exit means we cannot trust the table
    # (crash/timeout could leave partial output), so don't risk parsing it.
    if proc.returncode != 0:
        print(f"ERROR: runner exited {proc.returncode}", file=sys.stderr)
        return 2

    # Parse "module  pass  total  %" rows from the runner table.
    # The header row ("module  pass  total  %") can't match because pass/total
    # are not digits, so no name-based exclusion is needed — and excluding the
    # name "module" would wrongly drop the real node:module module.
    current = {}
    for line in proc.stdout.splitlines():
        m = re.match(r"^(\S+)\s+(\d+)\s+(\d+)\s+[\d.]+", line)
        if m:
            current[m.group(1)] = {"pass": int(m.group(2)), "total": int(m.group(3))}

    regressions, improvements = [], []
    for mod, floor in baseline.items():
        cur = current.get(mod)
        if cur is None:
            regressions.append(f"{mod}: MISSING from run (was {floor['pass']}/{floor['total']})")
            continue
        if cur["pass"] < floor["pass"]:
            regressions.append(
                f"{mod}: {cur['pass']}/{cur['total']} < floor {floor['pass']}/{floor['total']}  (-{floor['pass'] - cur['pass']})")
        elif cur["pass"] > floor["pass"]:
            improvements.append(f"{mod}: {cur['pass']}/{cur['total']}  (+{cur['pass'] - floor['pass']})")

    # Overall is derived, not stored (avoids cross-PR merge conflicts on a
    # shared aggregate). Compute it from the per-module floors at report time.
    bp = sum(m["pass"] for m in baseline.values())
    bt = sum(m["total"] for m in baseline.values())
    print("\n=== node-suite regression check ===")
    print(f"baseline floor overall: {bp}/{bt} ({round(100 * bp / bt, 1)}%)")
    if improvements:
        print("improvements (ratchet the baseline up):")
        for s in improvements:
            print("  + " + s)
    if regressions:
        print("REGRESSIONS:")
        for s in regressions:
            print("  ! " + s)
        return 1
    print("OK — no module dropped below its floor.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
