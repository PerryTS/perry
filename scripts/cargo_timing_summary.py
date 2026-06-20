#!/usr/bin/env python3
"""Summarize Cargo `--timings` output: print the slowest build units (#5422).

Cargo writes an HTML timing report to `target/cargo-timings/cargo-timing-*.html`
(and a `cargo-timing.html` symlink to the latest) whenever a build runs with
`--timings`. That HTML embeds the per-unit data as a `const UNIT_DATA = [...]`
JSON array. This script extracts it and prints the top-N units by wall-clock
duration so release build regressions are visible without weakening any
optimization settings.

Usage:
    cargo build --profile dist -p perry --timings
    scripts/cargo_timing_summary.py                 # latest report, top 15
    scripts/cargo_timing_summary.py --top 30
    scripts/cargo_timing_summary.py path/to/cargo-timing.html

Exit codes: 0 on success, 1 if no timing report was found.
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import re
import sys

DEFAULT_GLOB = "target/cargo-timings/cargo-timing-*.html"


def find_latest_report() -> str | None:
    # Prefer the stable `cargo-timing.html` pointer, else the newest timestamped one.
    pointer = "target/cargo-timings/cargo-timing.html"
    if os.path.exists(pointer):
        return pointer
    reports = glob.glob(DEFAULT_GLOB)
    if not reports:
        return None
    return max(reports, key=os.path.getmtime)


def extract_units(html: str) -> list[dict]:
    # Cargo embeds: const UNIT_DATA = [ {...}, ... ];
    m = re.search(r"UNIT_DATA\s*=\s*(\[.*?\]);", html, re.DOTALL)
    if not m:
        raise ValueError("could not find UNIT_DATA in the timing report")
    try:
        return json.loads(m.group(1))
    except json.JSONDecodeError as e:
        raise ValueError(f"malformed UNIT_DATA in timing report: {e}") from e


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("report", nargs="?", help="path to a cargo-timing*.html report")
    ap.add_argument("--top", type=int, default=15, help="how many slow units to show")
    args = ap.parse_args()

    report = args.report or find_latest_report()
    if not report or not os.path.exists(report):
        print(f"error: no cargo timing report found (looked for {DEFAULT_GLOB}).\n"
              f"       run a build with `--timings` first.", file=sys.stderr)
        return 1

    units = extract_units(open(report, encoding="utf-8").read())
    # Each unit has at least: name, version, duration (seconds), and usually
    # `rmeta_time` (time until dependents could start). Sort by total duration.
    units.sort(key=lambda u: u.get("duration", 0.0), reverse=True)

    total = sum(u.get("duration", 0.0) for u in units)
    print(f"# Cargo build timing summary  ({os.path.basename(report)})")
    print(f"# {len(units)} units, {total:.1f}s total compile time (sum, not wall-clock)\n")
    print(f"{'rank':>4}  {'seconds':>8}  {'rmeta':>7}  unit")
    print(f"{'----':>4}  {'-------':>8}  {'-----':>7}  ----")
    for i, u in enumerate(units[: args.top], 1):
        dur = u.get("duration", 0.0)
        rmeta = u.get("rmeta_time")
        rmeta_s = f"{rmeta:.1f}" if isinstance(rmeta, (int, float)) else "-"
        name = f"{u.get('name', '?')} v{u.get('version', '?')}"
        print(f"{i:>4}  {dur:>8.1f}  {rmeta_s:>7}  {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
