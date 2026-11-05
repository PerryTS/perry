#!/usr/bin/env python3
"""Merge sharded test262 subset reports and ratchet them against a baseline.

Every shard of the nightly `test262` job writes a `scripts/test262_subset.py`
report (`--full-corpus`). This merges them into one pass rate over every case,
comparable to other engines' published Test262 figures, and compares it with
`test-compat/test262/baseline.json`:

- a test that is not in the baseline's `known_failures` and now fails is a
  regression;
- a known failure that now passes is stale and must be removed, so a fix
  cannot silently regress later;
- `pass` must not fall below the baseline's `min_pass`, which also catches a
  test leaving the judged set (pass -> skip) or a lost shard.

    scripts/test262_ratchet.py --expect 8 shard-*/report.json
    scripts/test262_ratchet.py --update report.json   # rewrite the baseline
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
BASELINE = REPO_ROOT / "test-compat" / "test262" / "baseline.json"
BUCKETS = ("pass", "diff", "runtime-fail", "compile-fail", "skip")


def merge(reports: list[dict]) -> dict:
    pinned = {r["test262_pinned"] for r in reports}
    nodes = {r["node_runtime"] for r in reports}
    if len(pinned) != 1 or len(nodes) != 1:
        raise SystemExit(f"error: shards disagree on test262 {pinned} or node {nodes}")
    totals = {k: sum(r["totals"][k] for r in reports) for k in BUCKETS}
    judged = sum(totals.values())  # skips count: a case that cannot run fails
    failures = sorted(f["test"] for r in reports for f in r["failures"])
    return {
        "test262_pinned": pinned.pop(),
        "node_runtime": nodes.pop(),
        "totals": totals,
        "judged": judged,
        "pass_pct": round(100 * totals["pass"] / judged, 2) if judged else 0.0,
        "failures": failures,
    }


def check(agg: dict, base: dict) -> list[str]:
    errors = []
    for key in ("test262_pinned", "node_runtime"):
        if agg[key] != base[key]:
            errors.append(f"{key} is {agg[key]} but the baseline was taken with "
                          f"{base[key]}; re-baseline with --update")
    known, now = set(base["known_failures"]), set(agg["failures"])
    for t in sorted(now - known):
        errors.append(f"regression: {t}")
    for t in sorted(known - now):
        errors.append(f"stale known failure (now passes, remove it): {t}")
    if agg["totals"]["pass"] < base["min_pass"]:
        errors.append(f"pass {agg['totals']['pass']} < baseline min_pass {base['min_pass']}")
    return errors


def baseline_from(agg: dict) -> dict:
    return {
        "test262_pinned": agg["test262_pinned"],
        "node_runtime": agg["node_runtime"],
        "min_pass": agg["totals"]["pass"],
        "judged": agg["judged"],
        "pass_pct": agg["pass_pct"],
        "known_failures": agg["failures"],
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("reports", nargs="+", type=Path)
    ap.add_argument("--expect", type=int, help="refuse unless exactly N shard reports")
    ap.add_argument("--baseline", type=Path, default=BASELINE)
    ap.add_argument("--update", action="store_true", help="rewrite the baseline")
    args = ap.parse_args()

    if args.expect is not None and len(args.reports) != args.expect:
        print(f"error: expected {args.expect} shard reports, got {len(args.reports)}")
        return 1
    agg = merge([json.loads(p.read_text()) for p in args.reports])
    t = agg["totals"]
    line = (f"test262 {agg['test262_pinned'][:10]} (node {agg['node_runtime']}): "
            f"pass {t['pass']}/{agg['judged']} = {agg['pass_pct']}%  "
            f"(diff {t['diff']}, runtime-fail {t['runtime-fail']}, "
            f"compile-fail {t['compile-fail']}, skip {t['skip']})")
    print(line)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a") as f:
            f.write(f"### test262\n\n{line}\n")

    if args.update:
        args.baseline.write_text(json.dumps(baseline_from(agg), indent=1) + "\n")
        print(f"wrote {args.baseline}")
        return 0
    errors = check(agg, json.loads(args.baseline.read_text()))
    for e in errors:
        print(e)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
