#!/usr/bin/env python3
"""Verdict logic for the #7647 parse-then-churn layout-state gate.

WHY THIS EXISTS
---------------
#7643 measured that `PERRY_JSON_TAPE=0` + `PERRY_GC_FROMSPACE_SCAN=1` over a
parse-then-churn workload is a known-good end-to-end detector for the whole
layout-state family (#7630 / #7633 / #7635 / #7644): with the JSON
materialiser's finalize sabotaged to always claim `POINTER_FREE`, it reports
`dangling=8000 owners=4000` and the binary SIGBUSes; clean, it reports
`dangling=0` and exits 0. Nothing ran it in CI (#7647).

Promoting a check to a gate is not just wiring it into a workflow. Per
CLAUDE.md's "four ways a gate can be unable to fail", #4 is the dangerous
one: the gate runs but its subject never does. A parse-then-churn workload
that happens not to trigger a copying minor, or whose JSON stays on the lazy
tape despite `PERRY_JSON_TAPE=0`, would report a clean scan and mean nothing.
So this checker asserts THREE things, not one:

  1. CORRECTNESS  -- the fixture's own byte-exact comparison after the churn
     (`MISMATCHES 0`) AND the from-space scan found nothing (no offender
     line, which on the gate's own invocation manifests as an aborting
     nonzero exit -- see `scripts/gc_parse_churn_layout_gate.sh`).
  2. LIVENESS      -- at least one copying minor actually relocated objects
     (`copied_objects` summed across every `[gc-copy-minor] ran ...` line is
     nonzero). A run that never triggers the moving collector cannot fail
     however broken the layout state is.
  3. EAGERNESS     -- the from-space scan's own `objects=` census reached at
     least `--records` objects. `js_json_parse`'s Auto mode routes a
     top-level array in [1 KB, 16 MB) through the LAZY TAPE by default
     (json_tape, #7635's whole finding), which defers materializing each
     record until first read. `PERRY_JSON_TAPE=0` is supposed to force the
     direct (eager) parser for every call regardless of size or shape, but
     trusting the env var alone is exactly the kind of assumption #7635
     showed to be worth checking rather than asserting: a lazily-parsed
     cohort leaves only a handful of tape/lazy-array objects live at scan
     time, nowhere near `--records`. This is the "record count materialised
     before the churn, or an equivalent observable" #7647 asks for.

Usage
-----
    python3 scripts/gc_parse_churn_layout_check.py \\
        --exit-code N --stdout stdout.txt --stderr stderr.txt --records 4000

    python3 scripts/gc_parse_churn_layout_check.py --self-test

`--self-test` runs the verdict function against synthetic captures covering
every failure mode below, including the vacuous/lazy-tape shape -- proof this
checker can say no, not merely that it has not yet said no.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field

SUCCESS_SENTINEL = "PARSE_CHURN_LAYOUT_GATE_OK"
MISMATCH_RE = re.compile(r"^MISMATCHES (\d+)$", re.MULTILINE)
COPIED_OBJECTS_RE = re.compile(r"\[gc-copy-minor\] ran copied_objects=(\d+)")
SCAN_LINE_RE = re.compile(r"^\[gc-fromspace-scan (\S+)\] objects=(\d+)", re.MULTILINE)
OFFENDER_PHASES = {"OFFENDERS", "abort"}


@dataclass
class Verdict:
    ok: bool
    problems: list[str] = field(default_factory=list)

    def fail(self, msg: str) -> None:
        self.ok = False
        self.problems.append(msg)


def evaluate(exit_code: int, stdout: str, stderr: str, records: int) -> Verdict:
    v = Verdict(ok=True)

    if exit_code != 0:
        panic_line = ""
        for line in stderr.splitlines():
            if "gc from-space scan" in line or "panicked at" in line:
                panic_line = f" ({line.strip()})"
                break
        v.fail(
            f"process exited {exit_code}, expected 0{panic_line}. Under "
            f"PERRY_GC_FROMSPACE_SCAN_ABORT=1 a nonzero/signalled exit means "
            f"the scan found a surviving from-space reference -- a real "
            f"layout-state defect, not a harness problem."
        )

    if SUCCESS_SENTINEL not in stdout:
        v.fail(
            f"stdout never printed {SUCCESS_SENTINEL!r} -- the fixture did "
            f"not run to completion (crashed, threw, or was killed before "
            f"its own final assertion)."
        )

    m = MISMATCH_RE.search(stdout)
    if m is None:
        v.fail("stdout has no 'MISMATCHES <n>' line -- the fixture did not reach its own read-back check.")
    elif int(m.group(1)) != 0:
        v.fail(
            f"the fixture's own post-churn read-back found {m.group(1)} "
            f"corrupted record(s): a record's field value differs from what "
            f"was parsed, even though the process did not crash. Evacuation "
            f"copies rather than zeroes, so a stranded child can read back "
            f"stale-but-plausible bytes without ever faulting -- this is a "
            f"real defect the from-space scan can miss."
        )

    copied = [int(n) for n in COPIED_OBJECTS_RE.findall(stderr)]
    total_copied = sum(copied)
    if total_copied == 0:
        v.fail(
            "no copying minor relocated anything (sum of every "
            "'[gc-copy-minor] ran copied_objects=' line in stderr is 0). "
            "The subject of this gate -- the moving collector -- never ran, "
            "so a clean scan proves nothing (CLAUDE.md's 'four ways a gate "
            "can be unable to fail', #4). Check PERRY_GC_MOVING_LOOP_POLLS=1 "
            "was set at BOTH compile time and run time, and PERRY_GC_DIAG=1 "
            "at run time so the evidence line is even printed."
        )

    scan_lines = SCAN_LINE_RE.findall(stderr)
    if not scan_lines:
        v.fail(
            "stderr has no '[gc-fromspace-scan ...]' line at all -- the scan "
            "itself never ran. PERRY_GC_FROMSPACE_SCAN_ABORT=1 is supposed "
            "to imply PERRY_GC_FROMSPACE_SCAN=1 (that used to be false and "
            "was fixed; see fromspace_scan.rs's resolve_scan_knobs), so this "
            "means either that regressed, or no collection happened at all "
            "(see the liveness problem above, if also present)."
        )
    else:
        max_objects = max(int(n) for _phase, n in scan_lines)
        if max_objects < records:
            v.fail(
                f"the from-space scan's own census topped out at "
                f"{max_objects} live objects outside from-space, which is "
                f"fewer than the {records} records this fixture parses. "
                f"That means the record cohort was not eagerly materialised "
                f"before the churn -- PERRY_JSON_TAPE=0 did not force the "
                f"direct parser (or something else routed this workload "
                f"through the lazy tape), so the from-space scan measured an "
                f"empty-ish heap and a clean result would have meant "
                f"nothing. This is #7635's original vacuity, recurring."
            )
        offender_lines = [
            (phase, n) for phase, n in scan_lines if phase in OFFENDER_PHASES
        ]
        if offender_lines:
            phase, n = offender_lines[0]
            v.fail(
                f"stderr contains a '[gc-fromspace-scan {phase}]' line "
                f"reporting offenders (objects={n}) even though the process "
                f"exit code did not reflect it -- inspect the captured "
                f"stderr directly."
            )

    return v


def format_report(v: Verdict) -> str:
    if v.ok:
        return "PASS: parse-then-churn layout-state gate is clean and its subject was live."
    lines = ["FAIL: parse-then-churn layout-state gate"]
    for p in v.problems:
        lines.append(f"  - {p}")
    return "\n".join(lines)


# --------------------------------------------------------------------------- self-test


def _self_test() -> int:
    cases: list[tuple[str, int, str, str, int, bool]] = []

    ok_stdout = "BLOB_BYTES 245781\nPARSED_LENGTH 4000\nCHURN_TOUCH 240000\nMISMATCHES 0\n" + SUCCESS_SENTINEL + "\n"
    ok_stderr = (
        "[gc-copy-minor] ran copied_objects=537 copied_bytes=46112\n"
        "[gc-fromspace-scan clean] objects=6041 words=86354 fwd_owners_skipped=0 missing_rewrites=0 dangling=0 owners=0\n"
        "[gc-copy-minor] ran copied_objects=612 copied_bytes=51200\n"
        "[gc-fromspace-scan clean] objects=9210 words=120000 fwd_owners_skipped=0 missing_rewrites=0 dangling=0 owners=0\n"
    )
    cases.append(("clean run passes", 0, ok_stdout, ok_stderr, 4000, True))

    aborted_stderr = ok_stderr + (
        "[gc-fromspace-scan abort] objects=5052 words=86354 fwd_owners_skipped=3 "
        "missing_rewrites=0 dangling=1 owners=1\n"
        "thread '<unnamed>' panicked at crates/perry-runtime/src/gc/fromspace_scan.rs:351:5:\n"
        "gc from-space scan: 0 missing rewrite(s), 1 dangling reference(s) survived the rewrite pass\n"
    )
    truncated_stdout = "BLOB_BYTES 245781\nPARSED_LENGTH 4000\n"
    cases.append(("real dangling reference aborts the process -> FAIL", 134, truncated_stdout, aborted_stderr, 4000, False))

    corrupted_stdout = ok_stdout.replace("MISMATCHES 0", "MISMATCHES 3").replace(
        SUCCESS_SENTINEL + "\n", ""
    )
    cases.append(("silent corruption without a crash -> FAIL", 1, corrupted_stdout, ok_stderr, 4000, False))

    no_copy_stderr = "\n".join(
        line for line in ok_stderr.splitlines() if "gc-copy-minor" not in line
    )
    cases.append(("no copying minor ever ran -> FAIL (liveness)", 0, ok_stdout, no_copy_stderr, 4000, False))

    no_scan_stderr = "\n".join(
        line for line in ok_stderr.splitlines() if "gc-fromspace-scan" not in line
    )
    cases.append(("scan never ran at all -> FAIL (ABORT-alone-inert class)", 0, ok_stdout, no_scan_stderr, 4000, False))

    lazy_stderr = (
        "[gc-copy-minor] ran copied_objects=4 copied_bytes=512\n"
        "[gc-fromspace-scan clean] objects=9 words=88 fwd_owners_skipped=0 missing_rewrites=0 dangling=0 owners=0\n"
    )
    cases.append(("tape stayed lazy: scan sees ~9 objects not 4000 -> FAIL (eagerness/vacuity)", 0, ok_stdout, lazy_stderr, 4000, False))

    missing_sentinel_stdout = "BLOB_BYTES 245781\nPARSED_LENGTH 4000\nMISMATCHES 0\n"
    cases.append(("crashed after the mismatch check, before the sentinel -> FAIL", 1, missing_sentinel_stdout, ok_stderr, 4000, False))

    failures = []
    for name, exit_code, stdout, stderr, records, expect_ok in cases:
        v = evaluate(exit_code, stdout, stderr, records)
        if v.ok != expect_ok:
            failures.append(
                f"  - {name}: expected ok={expect_ok}, got ok={v.ok} ({v.problems})"
            )
        else:
            print(f"ok   {name}")

    if failures:
        print("SELF-TEST FAILED:")
        for f in failures:
            print(f)
        return 1
    print(f"\nSELF-TEST OK: {len(cases)} cases, including both directions "
          "(a real defect fails it; a clean+live run passes it).")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--exit-code", type=int)
    parser.add_argument("--stdout", help="path to captured stdout")
    parser.add_argument("--stderr", help="path to captured stderr")
    parser.add_argument("--records", type=int, help="expected record-cohort size")
    args = parser.parse_args()

    if args.self_test:
        return _self_test()

    missing = [
        name
        for name, val in (
            ("--exit-code", args.exit_code),
            ("--stdout", args.stdout),
            ("--stderr", args.stderr),
            ("--records", args.records),
        )
        if val is None
    ]
    if missing:
        parser.error(f"missing required argument(s): {', '.join(missing)} (or pass --self-test)")

    with open(args.stdout, encoding="utf-8", errors="replace") as f:
        stdout_text = f.read()
    with open(args.stderr, encoding="utf-8", errors="replace") as f:
        stderr_text = f.read()

    v = evaluate(args.exit_code, stdout_text, stderr_text, args.records)
    print(format_report(v))
    return 0 if v.ok else 1


if __name__ == "__main__":
    sys.exit(main())
