#!/usr/bin/env python3
"""Emit results/summary.txt — separates correctness regressions (output_match
== false) from perf-only signal. Closes the gap from #441 where wall-clock
"wins" on a binary producing the wrong output were reported as perf wins.

Reads results/results.json, writes results/summary.txt + prints the same
to stdout. Always exits 0 (the strict-mode gate is in run.sh — this script
just reports).
"""
import json
import statistics
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "results" / "results.json"
EXPECTED = ROOT / "results" / "expected.json"
SUMMARY = ROOT / "results" / "summary.txt"


def main() -> int:
    if not RESULTS.exists():
        print(f"missing {RESULTS} — run ./run.sh first")
        return 0

    data = json.loads(RESULTS.read_text())["rows"]
    expected_present = EXPECTED.exists()

    by_pair: dict[tuple, list] = defaultdict(list)
    for r in data:
        by_pair[(r["workload"], r["language"])].append(r)

    lines: list[str] = []
    lines.append("# honest_bench — output-correctness summary")
    lines.append(f"# {len(data)} measured rows across {len(by_pair)} (workload, language) pairs")
    lines.append(f"# expected.json: {'present' if expected_present else 'ABSENT (no checks ran)'}")
    lines.append("")

    correctness_bad: list[str] = []
    correctness_unchecked: list[str] = []
    perf_lines: list[str] = []

    for (workload, lang), rows in sorted(by_pair.items()):
        n = len(rows)
        ok_runs = [r for r in rows if r["exit_code"] == 0]
        ok_walls = [r["wall_ms"] for r in ok_runs]
        med = statistics.median(ok_walls) if ok_walls else None

        match_states = [r.get("output_match") for r in rows]
        n_match  = sum(1 for s in match_states if s is True)
        n_bad    = sum(1 for s in match_states if s is False)
        n_unchecked = sum(1 for s in match_states if s is None)

        med_str = f"{med:7.1f} ms" if med is not None else "    fail"
        line = f"{workload:24s} {lang:6s}  {med_str}  runs={n}  match={n_match}/{n}"
        if n_bad:
            reasons = sorted({r.get("output_match_reason", "")
                              for r in rows if r.get("output_match") is False})
            line += f"  ✗ MISMATCH ({n_bad} rows): {reasons[0][:120]}"
            correctness_bad.append(line)
        elif n_unchecked == n:
            line += "  (unchecked)"
            correctness_unchecked.append(line)
        else:
            perf_lines.append(line)

    if correctness_bad:
        lines.append("## CORRECTNESS REGRESSIONS — these binaries produced wrong output")
        lines.append("# Wall-time numbers for these rows MUST NOT be reported as perf wins.")
        lines.extend(correctness_bad)
        lines.append("")
    if correctness_unchecked:
        lines.append("## UNCHECKED — no expected entry, output not verified")
        lines.extend(correctness_unchecked)
        lines.append("")
    if perf_lines:
        lines.append("## PERF — output verified to match Bun reference")
        lines.extend(perf_lines)
        lines.append("")

    lines.append("# To turn mismatches into a hard CI failure: ./run.sh --strict-output")
    lines.append("# To refresh the reference (only when output semantics intentionally change):")
    lines.append("#   HONEST_BENCH_REFRESH_EXPECTED=1 ./run.sh")

    text = "\n".join(lines) + "\n"
    SUMMARY.write_text(text)
    print(text, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
