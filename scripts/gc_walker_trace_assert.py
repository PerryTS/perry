#!/usr/bin/env python3
"""Assert which native-stack-map walker actually ran, from a `PERRY_GC_TRACE=1`
stderr stream.

`PERRY_STACKMAP_WALKER` selects between three walks over the same roots, and a
run under the wrong one is indistinguishable by program output alone — every
mode is supposed to produce identical results, which is exactly why "it passed"
proves nothing about which one executed. The GC trace does distinguish them:

    mode      fp_walks   walks
    fast      > 0        > 0     x29 chain walk (aarch64 only)
    verify    > 0        > 0     chain walk cross-checked against the unwinder
    unwind    == 0       > 0     platform unwinder only

So `--require-fp-walks` is the liveness assert for `verify` (it is 0 the moment
the chain walk silently stops being used, and on a target where the chain walk
does not exist `verify` panics outright), and `--forbid-fp-walks` is the
liveness assert for `unwind` (nonzero means the mode did not take effect and
the arm was measuring `fast` all along).

`--require-locations` is the liveness assert for a walker that has never run
anywhere before (#7354, the Windows RtlVirtualUnwind arm): a walker that visits
zero frames still lets most probes print the right answer, because other root
sources cover them — so a green probe proves nothing. Non-zero
`frames_visited`, `records_matched` and `locations_visited` are what prove the
walker actually stepped mapped frames and enumerated their roots.

Usage:
    <program> 2> trace.err
    gc_walker_trace_assert.py trace.err --require-fp-walks
    gc_walker_trace_assert.py trace.err --forbid-fp-walks
    gc_walker_trace_assert.py trace.err --require-locations
"""

from __future__ import annotations

import argparse
import json
import sys


def totals(path: str) -> tuple[int, int, int, int, int]:
    fp_walks = walks = frames = records = locations = 0
    saw_event = False
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            stats = event.get("root_sources", {}).get("native_stack_maps")
            if not isinstance(stats, dict):
                continue
            saw_event = True
            fp_walks += stats.get("fp_walks", 0)
            walks += stats.get("walks", 0)
            frames += stats.get("frames_visited", 0)
            records += stats.get("records_matched", 0)
            locations += stats.get("locations_visited", 0)
    if not saw_event:
        sys.exit(
            f"::error::{path} carries no GC trace events with root_sources — "
            "PERRY_GC_TRACE=1 was not set, or no collection ran at all"
        )
    return fp_walks, walks, frames, records, locations


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("trace")
    ap.add_argument("--require-fp-walks", action="store_true")
    ap.add_argument("--forbid-fp-walks", action="store_true")
    ap.add_argument("--require-locations", action="store_true")
    args = ap.parse_args()

    fp_walks, walks, frames, records, locations = totals(args.trace)
    print(
        f"{args.trace}: walks={walks} fp_walks={fp_walks} "
        f"frames_visited={frames} records_matched={records} "
        f"locations_visited={locations}"
    )

    failures: list[str] = []
    if walks <= 0:
        failures.append(
            "the native stack-map walker never ran (walks == 0) — this arm "
            "asserted nothing about the walker"
        )
    if args.require_fp_walks and fp_walks <= 0:
        failures.append(
            "fp_walks == 0: the x29 chain walk did not run, so verify mode "
            "cross-checked nothing"
        )
    if args.forbid_fp_walks and fp_walks != 0:
        failures.append(
            f"fp_walks == {fp_walks}: PERRY_STACKMAP_WALKER=unwind did not take "
            "effect, the fast walk ran anyway"
        )
    if args.require_locations:
        zeroed = [
            (name, meaning)
            for name, value, meaning in (
                ("frames_visited", frames, "stepped a frame"),
                ("records_matched", records, "matched a mapped safepoint"),
                ("locations_visited", locations, "enumerated a root slot"),
            )
            if value <= 0
        ]
        for name, meaning in zeroed:
            failures.append(
                f"{name} == 0: the walker never {meaning} — green probes with "
                "zero telemetry mean the walker did not run, and other root "
                "sources covered for it (#7354)"
            )

    for message in failures:
        print(f"::error::{message}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
