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
# ANY `[gc-…]` diagnostic marker. These exist only under `PERRY_GC_DIAG=1`;
# `#gcmetric` summary lines are printed unconditionally, so their presence
# says the program ran but says nothing about whether diagnostics were on.
DIAG_MARKER = re.compile(r"\[gc-[a-z0-9-]+\]")


def check(text: str, probe: str, min_copied: int) -> tuple[int, list[str]]:
    """Return (exit_code, messages). Split out so `--self-test` can drive it."""
    copied = sum(int(m) for m in COPIED.findall(text))
    ran = text.count("[gc-copy-minor] ran ")
    ineligible = [m for m in ELIGIBLE.findall(text) if m[0] != "true"]

    if copied >= min_copied and ran > 0:
        return 0, [f"{probe}: evacuation live — {ran} copying minor(s), {copied} objects copied"]

    # ── Instrument off, or subject dead? ───────────────────────────────────
    # These are DIFFERENT failures and must not share a message. A trace with
    # no `[gc-…]` marker at all was produced without `PERRY_GC_DIAG=1`, so it
    # carries no evidence either way — blaming the collector there sends the
    # reader to debug a GC that may well have been evacuating the whole time.
    #
    # That is not hypothetical: it is #7970. `gc-native-roots`' in-process arm
    # omitted `PERRY_GC_DIAG=1` and this script reported "evacuated NOTHING (0
    # copying minors)" on every run since the arm was written. With the flag,
    # the same binary under the same GC env reported 75 copying minors and
    # 16277 objects copied.
    if not DIAG_MARKER.search(text):
        msgs = [
            f"::error::{probe}: this trace contains no `[gc-...]` diagnostic line at "
            f"all, so it cannot answer whether evacuation happened. "
            f"`PERRY_GC_DIAG=1` was NOT set on the run that produced it — that flag "
            f"is what emits `[gc-copy-minor]`, and this assert reads nothing else "
            f"(#7970).",
            "::error::Fix the RUN, not the collector: add `PERRY_GC_DIAG=1` to the "
            "command whose stderr is captured here. Only if the markers are present "
            "and still show zero copying minors is this a statement about the GC.",
        ]
        if not text.strip():
            msgs.append(
                "::error::(The trace is also completely empty — check the "
                "redirection, and that the program ran at all.)"
            )
        return 1, msgs

    msgs = [
        f"::error::{probe}: the forced-evacuation arm evacuated NOTHING "
        f"({ran} copying minors, {copied} objects copied). The arm is vacuous: "
        f"it proves the program ran, not that a moving collector did (#7336)."
    ]
    if not COPY_MINOR_ANY.search(text):
        msgs.append(
            "::error::No `[gc-copy-minor]` line at all — every collection in this "
            "run was a FULL cycle, which is what a probe that drives GC with `gc()` "
            "gets. PERRY_GC_FORCE_EVACUATE is read only on the MINOR path "
            "(#6942/#6946). Drive the minor path instead: PERRY_GC_HEAP_LIMIT=8 "
            "PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off."
        )
    if ineligible:
        kinds = sorted({f or "?" for _, f in ineligible})
        msgs.append(
            f"::error::Copying minor was ineligible; fallback(s): {', '.join(kinds)}. "
            "PERRY_CONSERVATIVE_STACK_SCAN=off is usually what makes it eligible (#7255)."
        )
    return 1, msgs


# Real shapes, abbreviated. `#gcmetric` prints WITHOUT PERRY_GC_DIAG; every
# `[gc-...]` marker prints only WITH it. That asymmetry is the whole discriminator.
NO_DIAG = (
    "#gcmetric heap_used_bytes=213400\n"
    "#gcmetric heap_total_bytes=12582912\n"
    "#gcmetric rss_bytes=24363008\n"
)
DIAG_LIVE = (
    "[gc-tenuring] promoted=12\n"
    "[gc-copy-minor] eligible=true\n"
    "[gc-copy-minor] ran from_space=1048576 copied_objects=16277\n"
)
DIAG_FULL_CYCLES_ONLY = "[gc-tenuring] promoted=12\n[gc-old-free] blocks=3\n"
DIAG_INELIGIBLE = "[gc-copy-minor] eligible=false fallback=conservative_scan\n"


def self_test() -> int:
    """Prove the checker separates 'instrument off' from 'nothing evacuated'."""
    failures = []

    rc, msgs = check(DIAG_LIVE, "p", 1)
    if rc != 0:
        failures.append(f"a live evacuation trace failed: {msgs}")

    rc, msgs = check(NO_DIAG, "p", 1)
    blob = " ".join(msgs)
    if rc == 0:
        failures.append("a trace with PERRY_GC_DIAG off was reported clean")
    elif "PERRY_GC_DIAG=1` was NOT set" not in blob:
        failures.append(f"diag-off was not named as the cause: {blob}")
    elif "evacuated NOTHING" in blob:
        failures.append("diag-off was blamed on the collector — the #7970 misdiagnosis")

    rc, msgs = check("", "p", 1)
    if rc == 0 or "completely empty" not in " ".join(msgs):
        failures.append("an empty trace was not called out")

    rc, msgs = check(DIAG_FULL_CYCLES_ONLY, "p", 1)
    blob = " ".join(msgs)
    if rc == 0:
        failures.append("diag-on with zero copying minors was reported clean")
    elif "evacuated NOTHING" not in blob or "PERRY_GC_DIAG=1` was NOT set" in blob:
        failures.append(f"diag-on/no-evacuation must blame the COLLECTOR: {blob}")

    rc, msgs = check(DIAG_INELIGIBLE, "p", 1)
    if rc == 0 or "ineligible" not in " ".join(msgs):
        failures.append("an ineligible copying minor was not reported")

    if failures:
        for f in failures:
            print(f"self-test FAILED: {f}", file=sys.stderr)
        return 1
    print("gc_evacuation_liveness_assert self-test: OK (5 directions)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("trace", nargs="?")
    ap.add_argument("--probe", default="<probe>")
    ap.add_argument("--min-copied", type=int, default=1)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return self_test()
    if not args.trace:
        ap.error("a trace path is required (or --self-test)")

    with open(args.trace, "r", errors="replace", encoding="utf-8") as fh:
        text = fh.read()
    rc, msgs = check(text, args.probe, args.min_copied)
    for m in msgs:
        print(m)
    return rc


if __name__ == "__main__":
    sys.exit(main())
