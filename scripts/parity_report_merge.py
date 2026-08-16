#!/usr/bin/env python3
"""Merge N sharded parity reports into one whole-suite report.

WHY: the `parity` job used to run the whole suite in ONE runner and was killed
by GitHub's 6-hour job cap on 2026-08-16 (run 31935729773, 11:44 -> 17:45,
`cancelled`) — the release gate could not complete even in principle. The job
is now sharded (`run_parity_tests.sh --shard N/M`, same round-robin partition
the gap suite uses), and the AGGREGATE gates — `parity_threshold_gate.py` and
`parity_matrix_trend.py` — run once in a fan-in job over the report this
script produces. They stay aggregate on purpose: a per-category minimum
evaluated on a shard's small slice would flap (a 62%-floor category with two
tests in a shard reads 50% on one failure), and the matrix trend's committed
baseline describes the whole suite. `parity_known_failures.py` is the one
gate that IS shard-safe by design ("not in this shard is never flagged"), so
it runs inside each shard.

Merge semantics:
  * `results`: concatenated. A test id appearing in two inputs is an ERROR —
    shards partition the suite disjointly, so a duplicate means two inputs
    were the same shard (or an artifact was downloaded twice).
  * `failures.{parity,compile,crash}`: unioned, sorted, deduplicated.
  * `summary`: counts summed; `parity_percentage` recomputed with the
    harness's own formula (pass / (pass + parity_fail + crash), in tenths).
  * `platform`: must agree across inputs; `generated_at`: max of inputs.

--expect N is REQUIRED and this script FAILS when given fewer inputs: a lost
shard artifact must not silently shrink the suite into a smaller, greener one
(the #6364/#7856 hazard: absence of evidence reading as passing).

Usage:
  scripts/parity_report_merge.py --expect 8 --output merged.json shard*/latest.json
  scripts/parity_report_merge.py --self-test
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


def load(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        data = json.load(fh)
    if not isinstance(data, dict) or not isinstance(data.get("results"), list):
        raise SystemExit(f"{path}: not a parity report (no results[])")
    return data


SUM_KEYS = (
    "parity_pass",
    "parity_fail",
    "compile_fail",
    "crash_fail",
    "node_fail",
    "skipped",
    "total_run",
)


def merge(reports: list[dict[str, Any]], sources: list[str]) -> dict[str, Any]:
    platforms = {r.get("platform") for r in reports}
    if len(platforms) != 1:
        raise SystemExit(f"platform mismatch across shard reports: {sorted(map(str, platforms))}")

    seen: dict[str, str] = {}
    results: list[dict[str, Any]] = []
    for rep, src in zip(reports, sources):
        for item in rep["results"]:
            if not isinstance(item, dict):
                continue
            tid = item.get("id") or item.get("test") or item.get("name")
            if tid is not None:
                if tid in seen:
                    raise SystemExit(
                        f"test id {tid!r} appears in both {seen[tid]} and {src} — "
                        f"shards partition disjointly, so two inputs cover the same shard"
                    )
                seen[tid] = src
            results.append(item)

    summary = {k: 0 for k in SUM_KEYS}
    for rep in reports:
        s = rep.get("summary") or {}
        for k in SUM_KEYS:
            v = s.get(k, 0)
            if not isinstance(v, int):
                raise SystemExit(f"summary.{k} is not an int in one input: {v!r}")
            summary[k] += v
    # The harness's own formula (run_parity_tests.sh): pass / (pass+fail+crash),
    # integer tenths. Reproduced exactly so a merged report of one shard is
    # byte-comparable with that shard's own report.
    denom = summary["parity_pass"] + summary["parity_fail"] + summary["crash_fail"]
    if denom > 0:
        tenths = summary["parity_pass"] * 1000 // denom
        summary["parity_percentage"] = float(f"{tenths // 10}.{tenths % 10}")
    else:
        summary["parity_percentage"] = 0.0

    failures: dict[str, list[str]] = {}
    for key in ("parity", "compile", "crash"):
        bucket: set[str] = set()
        for rep in reports:
            for name in (rep.get("failures") or {}).get(key, []) or []:
                if name:
                    bucket.add(name)
        failures[key] = sorted(bucket)

    return {
        "generated_at": max(str(r.get("generated_at", "")) for r in reports),
        "platform": next(iter(platforms)),
        "merged_from": len(reports),
        "summary": summary,
        "failures": failures,
        "results": results,
    }


# ---------------------------------------------------------------------------
def _rep(platform: str = "linux", ids: tuple[str, ...] = (), fails: tuple[str, ...] = ()) -> dict:
    return {
        "generated_at": "2026-08-16T00:00:00Z",
        "platform": platform,
        "summary": {
            "parity_pass": len(ids) - len(fails),
            "parity_fail": len(fails),
            "compile_fail": 0,
            "crash_fail": 0,
            "node_fail": 0,
            "skipped": 0,
            "total_run": len(ids),
        },
        "failures": {"parity": list(fails), "compile": [], "crash": []},
        "results": [
            {"id": i, "status": "parity_fail" if i in fails else "pass"} for i in ids
        ],
    }


def _self_test() -> int:
    fails: list[str] = []

    def check(name: str, cond: bool) -> None:
        if not cond:
            fails.append(name)

    m = merge([_rep(ids=("a", "b")), _rep(ids=("c",), fails=("c",))], ["s1", "s2"])
    check("results concatenated", len(m["results"]) == 3)
    check("counts summed", m["summary"]["parity_pass"] == 2 and m["summary"]["parity_fail"] == 1)
    check("pct recomputed (2/3 = 66.6)", m["summary"]["parity_percentage"] == 66.6)
    check("failures unioned", m["failures"]["parity"] == ["c"])

    def raises(fn) -> bool:
        try:
            fn()
        except SystemExit:
            return True
        return False

    check("duplicate id is an error", raises(lambda: merge([_rep(ids=("a",)), _rep(ids=("a",))], ["s1", "s2"])))
    check("platform mismatch is an error", raises(lambda: merge([_rep("linux"), _rep("macos")], ["s1", "s2"])))
    check(
        "single-shard merge preserves the harness pct formula",
        merge([_rep(ids=("a", "b", "c"), fails=("c",))], ["s1"])["summary"]["parity_percentage"] == 66.6,
    )

    if fails:
        print("parity_report_merge --self-test FAILED:", file=sys.stderr)
        for f in fails:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("parity_report_merge --self-test: OK (7 cases)")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("inputs", nargs="*", type=Path)
    ap.add_argument("--expect", type=int, help="exact number of shard reports required")
    ap.add_argument("--output", type=Path)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args(argv)

    if args.self_test:
        return _self_test()
    if not args.expect or not args.output or not args.inputs:
        ap.error("--expect, --output and at least one input are required")
    if len(args.inputs) != args.expect:
        raise SystemExit(
            f"expected exactly {args.expect} shard reports, got {len(args.inputs)}: "
            f"{[str(p) for p in args.inputs]} — a missing shard artifact must fail "
            f"the merge, not shrink the suite"
        )

    merged = merge([load(p) for p in args.inputs], [str(p) for p in args.inputs])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as fh:
        json.dump(merged, fh, indent=1)
        fh.write("\n")
    s = merged["summary"]
    print(
        f"merged {len(args.inputs)} shard reports -> {args.output}: "
        f"{s['parity_pass']} pass / {s['parity_fail']} parity_fail / "
        f"{s['compile_fail']} compile_fail / {s['crash_fail']} crash / "
        f"{s['node_fail']} node_fail / {s['skipped']} skipped = {s['parity_percentage']}%"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
