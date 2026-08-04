#!/usr/bin/env python3
"""Assert on a `perry compile --statepoint-report=json` report.

The report is written to stderr, interleaved with linker warnings and driver
chatter, so this reads the whole stream and decodes the first JSON object in
it rather than assuming the file is pure JSON.

Exists so the `gc-native-roots` CI arms can assert *their subject was live*
rather than merely that nothing threw (CLAUDE.md, "four ways a gate can be
unable to fail", #4). Each mode has a signature in this report that the other
modes cannot produce:

  * RS4GC (`PERRY_RS4GC=1`)   -> every record `"backend": "rs4gc"`, and
    `gc_map.records > 0`. The explicit bridge is deleted, so there is no
    second backend to fall back to; `--only-backend` still answers "did it
    run *everywhere*", which is a different question from "did it run".
  * safepoint density        -> `gc_map.records` and `gc_map.roots`, which is
    what `--print` is for.

**Fields are looked up in `totals` first, then `gc_map`.** That split is not
cosmetic. `totals` is counted at IR-emission time; `gc_map` comes from the
compact-map rewrite parsing the assembly LLVM actually emitted. Since RS4GC —
not Perry — decides which calls become safepoints, `gc_map` is the only honest
source for safepoint counts. #7348 deleted the emission-time writers along with
the bridge and the report went on printing `0 statepoints emitted` for every
compile; `gc_map.modules == 0` now means "not measured" and is distinguishable
from a real zero.

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

    gc_map = report.get("gc_map", {})

    def lookup(field: str):
        """`totals` first, then `gc_map` — see the module docstring."""
        if field in totals:
            return totals[field]
        return gc_map.get(field)

    def where(field: str) -> str:
        """Which section answered, so the printed line is not misleading."""
        return "totals" if field in totals else "gc_map"

    # A report whose map never reported cannot answer any safepoint question,
    # and must not be allowed to satisfy a --require-zero by absence.
    if (args.require_positive or args.require_zero or args.print_field) and not gc_map.get(
        "modules"
    ):
        failures.append(
            "gc_map.modules == 0: the compact-map rewrite never reported, so "
            "safepoint counts are NOT MEASURED rather than zero"
        )

    for field in args.require_positive:
        value = lookup(field)
        if not isinstance(value, int):
            failures.append(f"{field} missing from the report (checked totals and gc_map)")
        elif value <= 0:
            failures.append(f"{where(field)}.{field} == {value}, expected > 0 (the mode did nothing)")
        else:
            print(f"{where(field)}.{field} = {value}")

    for field in args.require_zero:
        value = lookup(field)
        if not isinstance(value, int):
            failures.append(f"{field} missing from the report (checked totals and gc_map)")
        elif value != 0:
            failures.append(f"{where(field)}.{field} == {value}, expected 0")
        else:
            print(f"{where(field)}.{field} = 0")

    if args.print_field:
        value = lookup(args.print_field)
        if not isinstance(value, int):
            sys.exit(f"::error::{args.print_field} missing from the report (checked totals and gc_map)")
        print(value)

    for message in failures:
        print(f"::error::{message}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
