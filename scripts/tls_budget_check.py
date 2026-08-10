#!/usr/bin/env python3
"""Verdict logic for the `_tlv_get_addr` budget gate (#7469).

WHY THIS EXISTS
===============

On Darwin every `thread_local!` access is an out-of-line call to
`_tlv_get_addr`. `crates/perry-runtime/src/tls_hot.rs` collapses those calls to
loads, and it has done so three times: measured 0% of `churn_alloc` after
#7565, 8-9% a while later, 11% on `interp`/`retain`, and 20.5% of `asyncpipe`
by v0.5.1434. Nothing watched it. This is what watches it.

THE VACUITY THAT MATTERS HERE
=============================

The obvious gate — profile `churn_alloc`, assert `_tlv_get_addr` is small —
passes forever while the real cost grows, because `churn`'s thread-locals are
exactly the sixteen the named-field cache was curated for. It would be a gate
that cannot fail, of CLAUDE.md's fourth kind: green because its subject never
ran.

So this checker refuses to return a verdict unless the run proves three things:

1. The profile is real: enough samples, and enough *distinct* perry-runtime
   symbols that it is a broad runtime workload rather than a tight loop with
   one inlined allocation site.
2. The hot-cache mechanism was live: `PERRY_TLS_HOT_STATS=1` reports
   `direct_tsd=1` (otherwise `hot()` is itself paying `_tlv_get_addr`, the
   whole mechanism is inert, and a low share would mean the program resolved
   nothing) and `claimed` above a floor.
3. The program exercised paths *outside* the sixteen named slots. `claimed`
   counts generic-slot declarations that were actually resolved; `churn` drives
   it to a handful, `asyncpipe` and `interp` to dozens. A subject that cannot
   clear the floor is the wrong subject, and says so.

`--self-test` drives every one of those rejections plus the budget itself, so
the checker cannot quietly stop being able to say no.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter

LINE_RE = re.compile(r"^([ +!:|]*)(\d+) (.*?)  \(in ([^)]*)\)")
STATS_RE = re.compile(
    r"\[tls-hot\] claimed=(\d+) published=(\d+) capacity=(\d+) direct_tsd=(\d+)"
)
TARGET = "_tlv_get_addr"


class Profile:
    def __init__(self, root: int, target: int, symbols: Counter, callers: Counter):
        self.root = root
        self.target = target
        self.symbols = symbols
        self.callers = callers

    @property
    def share(self) -> float:
        return 100.0 * self.target / self.root if self.root else 0.0


def parse_report(text: str) -> Profile:
    stack: list[tuple[int, str]] = []
    callers: Counter = Counter()
    symbols: Counter = Counter()
    root = 0
    target = 0
    started = False
    for raw in text.splitlines():
        if raw.startswith("Call graph:"):
            started = True
            continue
        if not started:
            continue
        if raw.startswith("Binary Images:") or raw.startswith("Total number"):
            break
        m = LINE_RE.match(raw)
        if not m:
            continue
        indent, count, sym = len(m.group(1)), int(m.group(2)), m.group(3)
        while stack and stack[-1][0] >= indent:
            stack.pop()
        if not stack and sym == "start":
            root += count
        if "perry_runtime" in sym or sym.startswith("js_"):
            symbols[sym] += count
        if sym.startswith(TARGET):
            target += count
            callers[stack[-1][1] if stack else "<root>"] += count
        stack.append((indent, sym))
    return Profile(root, target, symbols, callers)


def parse_stats(text: str) -> dict[str, int] | None:
    m = STATS_RE.search(text)
    if not m:
        return None
    return {
        "claimed": int(m.group(1)),
        "published": int(m.group(2)),
        "capacity": int(m.group(3)),
        "direct_tsd": int(m.group(4)),
    }


def check(
    report: str,
    stats_text: str,
    budget: float,
    label: str,
    min_samples: int,
    min_symbols: int,
    min_claimed: int,
) -> tuple[bool, list[str], Profile | None]:
    problems: list[str] = []
    profile = parse_report(report)

    if profile.root < min_samples:
        problems.append(
            f"only {profile.root} root samples (need >= {min_samples}). "
            f"`sample` did not attach for long enough, or the program exited "
            f"first — a low share here would mean nothing."
        )
    if len(profile.symbols) < min_symbols:
        problems.append(
            f"only {len(profile.symbols)} distinct perry-runtime symbols "
            f"(need >= {min_symbols}). Either the binary is stripped — build "
            f"with PERRY_DEBUG_SYMBOLS=1 — or this is a narrow loop, not the "
            f"broad workload this budget is about."
        )

    stats = parse_stats(stats_text)
    if stats is None:
        problems.append(
            "no `[tls-hot]` line: run the program with PERRY_TLS_HOT_STATS=1. "
            "Without it there is no evidence the hot-slot cache was even live."
        )
    else:
        if stats["direct_tsd"] != 1:
            problems.append(
                "direct_tsd=0: `hot()` fell back to `_tlv_get_addr`, so the "
                "cache is inert and this measurement is about a configuration "
                "nobody ships."
            )
        if stats["claimed"] < min_claimed:
            problems.append(
                f"claimed={stats['claimed']} generic slots (need >= "
                f"{min_claimed}). This subject barely touches thread-locals "
                f"outside the sixteen named fields, which makes it the wrong "
                f"subject: it would pass this budget forever while the real "
                f"cost grew elsewhere."
            )
        if stats["claimed"] >= stats["capacity"]:
            problems.append(
                f"claimed={stats['claimed']} reached capacity "
                f"{stats['capacity']}: declarations past the ceiling silently "
                f"fall back to `_tlv_get_addr`."
            )

    over_budget = profile.share > budget
    if over_budget:
        problems.append(
            f"{TARGET} is {profile.share:.1f}% of {label} "
            f"({profile.target}/{profile.root} samples), budget {budget:.1f}%."
        )
    return (not problems), problems, profile


def self_test() -> int:
    """Every rejection above, driven in both directions."""

    def report(tlv: int, root: int, symbols: int) -> str:
        lines = ["Call graph:", f"      {root} start  (in p) + 1  [0x1]"]
        for i in range(symbols):
            lines.append(
                f"      + {max(root // max(symbols, 1), 1)} "
                f"_RNvNtCs_13perry_runtime3fn{i}  (in p) + 1  [0x2]"
            )
        lines.append(f"      + {tlv} {TARGET}  (in libdyld.dylib) + 4  [0x3]")
        return "\n".join(lines)

    good_stats = "[tls-hot] claimed=60 published=55 capacity=768 direct_tsd=1"
    failures = []

    ok, _, prof = check(report(100, 10000, 40), good_stats, 5.0, "x", 2000, 20, 20)
    if not ok:
        failures.append("a clean 1.0% run was rejected")
    if prof is None or abs(prof.share - 1.0) > 0.01:
        failures.append("share arithmetic is wrong")

    cases = [
        ("over budget", report(2000, 10000, 40), good_stats, 5.0, 2000, 20, 20),
        ("too few samples", report(10, 500, 40), good_stats, 5.0, 2000, 20, 20),
        ("stripped/narrow", report(10, 10000, 3), good_stats, 5.0, 2000, 20, 20),
        ("no stats line", report(10, 10000, 40), "", 5.0, 2000, 20, 20),
        (
            "inert cache",
            report(10, 10000, 40),
            "[tls-hot] claimed=60 published=55 capacity=768 direct_tsd=0",
            5.0,
            2000,
            20,
            20,
        ),
        (
            "covered subject",
            report(10, 10000, 40),
            "[tls-hot] claimed=3 published=3 capacity=768 direct_tsd=1",
            5.0,
            2000,
            20,
            20,
        ),
        (
            "slots exhausted",
            report(10, 10000, 40),
            "[tls-hot] claimed=768 published=700 capacity=768 direct_tsd=1",
            5.0,
            2000,
            20,
            20,
        ),
    ]
    for name, rep, stats, budget, ms, msym, mc in cases:
        ok, _, _ = check(rep, stats, budget, "x", ms, msym, mc)
        if ok:
            failures.append(f"{name}: accepted a run it must reject")

    for f in failures:
        print(f"SELF-TEST FAILED: {f}", file=sys.stderr)
    if failures:
        return 1
    print(f"self-test: the checker rejects all {len(cases)} failure modes")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--report", help="a `sample` report")
    ap.add_argument("--stats", help="stderr of the PERRY_TLS_HOT_STATS=1 run")
    ap.add_argument("--budget", type=float, default=5.0, help="max %% of root samples")
    ap.add_argument("--label", default="the program")
    ap.add_argument("--min-samples", type=int, default=2000)
    ap.add_argument("--min-symbols", type=int, default=20)
    ap.add_argument(
        "--min-claimed",
        type=int,
        default=20,
        help="floor on generic slots the subject must actually resolve",
    )
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        return self_test()
    if not args.report or not args.stats:
        ap.error("--report and --stats are required")

    report = open(args.report).read()
    stats = open(args.stats).read()
    ok, problems, profile = check(
        report,
        stats,
        args.budget,
        args.label,
        args.min_samples,
        args.min_symbols,
        args.min_claimed,
    )
    assert profile is not None
    print(f"=== {args.label}")
    print(f"    root samples : {profile.root}")
    print(f"    {TARGET:<13}: {profile.target} ({profile.share:.1f}%)  budget {args.budget:.1f}%")
    print(f"    stats        : {parse_stats(stats)}")
    if profile.callers:
        print("    remaining callers:")
        for sym, n in profile.callers.most_common(8):
            print(f"      {n:6d}  {sym}")
    if ok:
        print(f"    VERDICT      : PASS")
        return 0
    print("    VERDICT      : FAIL")
    for p in problems:
        print(f"      - {p}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
