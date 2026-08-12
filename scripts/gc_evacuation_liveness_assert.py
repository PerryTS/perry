#!/usr/bin/env python3
"""Assert a forced-evacuation run actually evacuated something (#7336).

`PERRY_GC_FORCE_EVACUATE=1` is read only on the *minor* path. A probe that
drives collection with `gc()` gets a full mark-sweep, which evacuates nothing —
the run is green and the arm measured no moving collector at all.

(Until #7558 that path was *also* behind a forced conservative scan, which the
`[gc-scan-fallback] site=manual_collect` line below used to detect. It no longer
prints, so the detector is the trigger kind instead: `[gc-copy-minor]` lines are
absent entirely when the only collections were full mark-sweeps. The underlying
hazard is unchanged — `gc()` is still a FULL cycle and
`PERRY_GC_FORCE_EVACUATE` is still read only on the minor path.)

That is not hypothetical: it is #6942/#6946, which CLAUDE.md records as costing
months of "passes under evacuation" that meant nothing. The `gc-native-roots`
evacuation arm was repeating it — `copied_objects` and `moved_objects` were 0 on
every probe, while `--require-fp-walks` passed because it asserts that a walk
*happened*, not that it *found* anything.

So this asserts the subject was live: at least one copying minor ran, and it
copied at least one object. Reads `PERRY_GC_DIAG=1` output on stderr.

    gc_evacuation_liveness_assert.py trace.err --probe 01_nursery_churn
"""
from __future__ import annotations

import argparse
import re
import sys

COPIED = re.compile(r"copied_objects=(\d+)")
ELIGIBLE = re.compile(r"\[gc-copy-minor\] eligible=(\w+)(?: fallback=(\S+))?")
# "This run's collections were all FULL cycles." Before #7558 the tell was the
# `site=manual_collect` scan-fallback line; explicit `gc()` no longer forces the
# scan, so that line is gone and the tell is the absence of any copying-minor
# line at all.
COPY_MINOR_ANY = re.compile(r"\[gc-copy-minor\]")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("trace")
    ap.add_argument("--probe", default="<probe>")
    ap.add_argument("--min-copied", type=int, default=1)
    args = ap.parse_args()

    text = open(args.trace, "r", errors="replace").read()
    copied = sum(int(m) for m in COPIED.findall(text))
    ran = text.count("[gc-copy-minor] ran ")
    ineligible = [m for m in ELIGIBLE.findall(text) if m[0] != "true"]

    if copied >= args.min_copied and ran > 0:
        print(f"{args.probe}: evacuation live — {ran} copying minor(s), {copied} objects copied")
        return 0

    print(f"::error::{args.probe}: the forced-evacuation arm evacuated NOTHING "
          f"({ran} copying minors, {copied} objects copied). The arm is vacuous: "
          f"it proves the program ran, not that a moving collector did (#7336).")
    if not COPY_MINOR_ANY.search(text):
        print("::error::No `[gc-copy-minor]` line at all — every collection in this "
              "run was a FULL cycle, which is what a probe that drives GC with `gc()` "
              "gets. PERRY_GC_FORCE_EVACUATE is read only on the MINOR path "
              "(#6942/#6946). Drive the minor path instead: PERRY_GC_HEAP_LIMIT=8 "
              "PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off.")
    if ineligible:
        kinds = sorted({f or '?' for _, f in ineligible})
        print(f"::error::Copying minor was ineligible; fallback(s): {', '.join(kinds)}. "
              "PERRY_CONSERVATIVE_STACK_SCAN=off is usually what makes it eligible (#7255).")
    return 1


if __name__ == "__main__":
    sys.exit(main())
