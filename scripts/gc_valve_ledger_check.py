#!/usr/bin/env python3
"""Gate: no allocation-point GC valve fired (RFC deferred collection, decision 5).

Every Perry binary run with ``PERRY_GC_VALVE_LEDGER=<file>`` appends exactly one
line to ``<file>`` at exit (``perry-runtime/src/gc/alloc_point.rs``)::

    v1 exe=<name> pid=<n> valve_fires=<n> parked_valve_fires=<n> d2_violations=<n> ...

This script fails when

* any line records ``valve_fires`` (the nursery slack valve),
  ``parked_valve_fires`` (a budgeted cycle's root phase served at an
  allocation point) or ``d2_violations`` above zero — the valve is the one
  allocation-point collection D2 permits, and it must stay exceptional; or
* the ledger has fewer lines than ``--expect-min`` — the counter that proves
  the check ran. A harness that stopped exporting the variable, or a binary
  whose exit path skipped the teardown funnel, would otherwise read as clean.

``--expect-min-from-report`` takes the minimum from a parity report's
``summary.parity_pass``: every test that passed ran a Perry binary to a normal
exit, so each must have written a line.

``OldReclaimAllocPoint`` is reported (``old_reclaim_alloc_point=``), never
gated: decision 1 keeps that arm at the allocation point.
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path

GATED = ("valve_fires", "parked_valve_fires", "d2_violations")


def parse_line(line: str) -> dict[str, str]:
    fields = {}
    for token in line.split():
        if "=" in token:
            key, value = token.split("=", 1)
            fields[key] = value
    return fields


def check(lines: list[str], expect_min: int) -> list[str]:
    errors = []
    records = [line for line in lines if line.startswith("v1 ")]
    malformed = [line for line in lines if line.strip() and not line.startswith("v1 ")]
    for line in malformed:
        errors.append(f"malformed ledger line: {line!r}")
    if len(records) < expect_min:
        errors.append(
            f"only {len(records)} ledger line(s), expected at least {expect_min}: the "
            "valve check did not run for every binary (is PERRY_GC_VALVE_LEDGER "
            "reaching the tests?)"
        )
    old_reclaim = 0
    for line in records:
        fields = parse_line(line)
        for key in GATED:
            try:
                value = int(fields.get(key, "missing"))
            except ValueError:
                errors.append(f"ledger line lacks an integer {key}: {line!r}")
                continue
            if value > 0:
                errors.append(f"{key}={value} in {fields.get('exe', '?')}: {line.strip()}")
        try:
            old_reclaim += int(fields.get("old_reclaim_alloc_point", "0"))
        except ValueError:
            pass
    print(
        f"gc-valve-ledger: {len(records)} binaries checked, "
        f"old_reclaim_alloc_point total={old_reclaim} (reported, not gated)"
    )
    return errors


def expect_min_from_report(path: Path) -> int:
    report = json.loads(path.read_text(encoding="utf-8"))
    return int(report["summary"]["parity_pass"])


def self_test() -> int:
    clean = "v1 exe=a pid=1 valve_fires=0 parked_valve_fires=0 d2_violations=0 old_reclaim_alloc_point=2\n"
    fired = "v1 exe=b pid=2 valve_fires=1 parked_valve_fires=0 d2_violations=0\n"
    parked = "v1 exe=c pid=3 valve_fires=0 parked_valve_fires=4 d2_violations=0\n"
    assert check([clean, clean], 2) == []
    assert check([clean, fired], 2), "a fired valve must fail"
    assert check([clean, parked], 1), "a fired parked-cycle valve must fail"
    assert check([clean], 2), "too few lines must fail (the check did not run)"
    assert check([], 0) == [], "an explicit zero minimum with no lines is allowed"
    assert check(["garbage\n"], 0), "a malformed line must fail"
    with tempfile.TemporaryDirectory() as tmp:
        report = Path(tmp) / "latest.json"
        report.write_text(json.dumps({"summary": {"parity_pass": 7}}), encoding="utf-8")
        assert expect_min_from_report(report) == 7
    print("gc_valve_ledger_check self-test: ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ledger", type=Path)
    parser.add_argument("--expect-min", type=int, default=None)
    parser.add_argument("--expect-min-from-report", type=Path, default=None)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.ledger is None:
        parser.error("--ledger is required")
    if args.expect_min is None and args.expect_min_from_report is None:
        parser.error("one of --expect-min / --expect-min-from-report is required")
    expect_min = args.expect_min or 0
    if args.expect_min_from_report is not None:
        expect_min = max(expect_min, expect_min_from_report(args.expect_min_from_report))
    if expect_min < 1:
        print("gc-valve-ledger: refusing a minimum of 0 — the gate could not fail", file=sys.stderr)
        return 1
    lines = (
        args.ledger.read_text(encoding="utf-8").splitlines(keepends=True)
        if args.ledger.exists()
        else []
    )
    errors = check(lines, expect_min)
    for error in errors:
        print(f"gc-valve-ledger: FAIL: {error}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
