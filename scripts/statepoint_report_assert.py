#!/usr/bin/env python3
"""Assert on a `perry compile --statepoint-report=json` report.

The report is written to stderr, interleaved with linker warnings and driver
chatter, so this reads the whole stream and decodes the first JSON object in
it rather than assuming the file is pure JSON.

Exists so the `gc-native-roots` CI arms can assert *their subject was live*
rather than merely that nothing threw (CLAUDE.md, "four ways a gate can be
unable to fail", #4). Each mode has a signature in this report that the other
modes cannot produce:

  * explicit statepoint bridge -> every record `"backend": "statepoint"`,
    `statepoints > 0`, `plain_stack_maps == 0`, `statepoint_fallbacks == 0`
  * RS4GC (`PERRY_RS4GC=1`)   -> every record `"backend": "rs4gc"`.  RS4GC
    bails per function to the explicit bridge on any unrecognised root-alloca
    shape, so "did it run" and "did it run everywhere" are different
    questions and only `--only-backend` answers the second.
  * safepoint-only contract   -> `skipped_non_safepoints` strictly up and
    `statepoints` strictly down against the same build without it, which is
    what `--print` is for.

Usage:
    statepoint_report_assert.py REPORT [--only-backend NAME]
                               [--require-positive FIELD]...
                               [--require-zero FIELD]...
                               [--print FIELD]
"""

from __future__ import annotations

import argparse
import json
import sys


def load(path: str) -> dict:
    text = open(path, encoding="utf-8", errors="replace").read()
    start = text.find("{")
    if start < 0:
        sys.exit(f"::error::{path} contains no JSON report — was --statepoint-report=json passed?")
    try:
        report, _ = json.JSONDecoder().raw_decode(text[start:])
    except json.JSONDecodeError as exc:
        sys.exit(f"::error::{path} does not decode as a statepoint report: {exc}")
    if "totals" not in report or "functions" not in report:
        sys.exit(f"::error::{path} is JSON but not a statepoint report (no totals/functions)")
    return report


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("report")
    ap.add_argument("--only-backend")
    ap.add_argument("--require-positive", action="append", default=[])
    ap.add_argument("--require-zero", action="append", default=[])
    ap.add_argument("--print", dest="print_field")
    args = ap.parse_args()

    report = load(args.report)
    totals = report["totals"]
    functions = report["functions"]
    failures: list[str] = []

    if args.only_backend is not None:
        if not functions:
            failures.append(
                f"no function records at all — the {args.only_backend} lowering never ran"
            )
        else:
            counts: dict[str, int] = {}
            for record in functions:
                backend = record.get("backend", "<missing>")
                counts[backend] = counts.get(backend, 0) + 1
            other = {k: v for k, v in counts.items() if k != args.only_backend}
            if args.only_backend not in counts:
                failures.append(
                    f"no function used backend {args.only_backend!r}; saw {counts}"
                )
            elif other:
                failures.append(
                    f"{sum(other.values())} function(s) fell back off backend "
                    f"{args.only_backend!r}: {other}"
                )
            else:
                print(f"backend {args.only_backend}: {counts[args.only_backend]} function(s)")

    for field in args.require_positive:
        value = totals.get(field)
        if not isinstance(value, int):
            failures.append(f"totals.{field} missing from the report")
        elif value <= 0:
            failures.append(f"totals.{field} == {value}, expected > 0 (the mode did nothing)")
        else:
            print(f"totals.{field} = {value}")

    for field in args.require_zero:
        value = totals.get(field)
        if not isinstance(value, int):
            failures.append(f"totals.{field} missing from the report")
        elif value != 0:
            failures.append(f"totals.{field} == {value}, expected 0")
        else:
            print(f"totals.{field} = 0")

    if args.print_field:
        value = totals.get(args.print_field)
        if not isinstance(value, int):
            sys.exit(f"::error::totals.{args.print_field} missing from the report")
        print(value)

    for message in failures:
        print(f"::error::{message}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
