#!/usr/bin/env python3
"""Compare semantic benchmark stdout against a reference stdout file.

The verifier ignores a suite fixture's explicitly declared measurement labels
and compares the remaining stable ``key:value`` or ``key=value`` lines.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


BENCHMARKS_DIR = Path(__file__).resolve().parent
if str(BENCHMARKS_DIR) not in sys.path:
    sys.path.insert(0, str(BENCHMARKS_DIR))

from benchmark_time import timing_contract


ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")
KV_RE = re.compile(r"^([A-Za-z0-9_(). -]+?)\s*([:=])\s*(.*?)\s*$")


def _strip_ansi(value: str) -> str:
    return ANSI_RE.sub("", value)


def _parse_kv(line: str) -> tuple[str, str, str] | None:
    match = KV_RE.match(line)
    if not match:
        return None
    key, sep, value = match.groups()
    return key.strip(), sep, value.strip()


def semantic_lines_from_text(text: str, benchmark: str) -> list[str]:
    lines: list[str] = []
    ignored_labels = timing_contract(benchmark).measurement_labels

    for raw_line in text.splitlines():
        line = _strip_ansi(raw_line).strip()
        if not line:
            continue

        kv = _parse_kv(line)
        if not kv:
            continue

        key, _, value = kv
        if key in ignored_labels:
            continue

        lines.append(f"{key}:{value}")

    return lines


def _read_text(path: str | Path) -> str:
    return Path(path).read_text(encoding="utf-8", errors="replace")


def _values_by_key(lines: list[str]) -> dict[str, list[str]]:
    values: dict[str, list[str]] = {}
    for line in lines:
        key, _, value = line.partition(":")
        values.setdefault(key, []).append(value)
    return values


def _mismatch_reason(expected_lines: list[str], actual_lines: list[str]) -> str:
    if not actual_lines:
        return "missing required semantic lines: " + ", ".join(expected_lines)

    expected_by_key = _values_by_key(expected_lines)
    actual_by_key = _values_by_key(actual_lines)
    parts: list[str] = []

    mismatched_keys = []
    for key in sorted(set(expected_by_key) & set(actual_by_key)):
        if expected_by_key[key] != actual_by_key[key]:
            mismatched_keys.append(
                f"{key}: expected {expected_by_key[key]!r}, actual {actual_by_key[key]!r}"
            )
    if mismatched_keys:
        parts.append("mismatched " + "; ".join(mismatched_keys))

    missing_keys = sorted(set(expected_by_key) - set(actual_by_key))
    if missing_keys:
        parts.append("missing keys " + ", ".join(missing_keys))

    extra_keys = sorted(set(actual_by_key) - set(expected_by_key))
    if extra_keys:
        parts.append("unexpected keys " + ", ".join(extra_keys))

    if not parts:
        parts.append(
            "semantic line order/count differs: "
            f"expected {expected_lines!r}, actual {actual_lines!r}"
        )

    return "; ".join(parts)


def compare_stdout_files(
    *,
    expected_path: str | Path,
    actual_path: str | Path,
    benchmark: str,
    reference: str = "node",
) -> dict[str, Any]:
    expected_lines = semantic_lines_from_text(_read_text(expected_path), benchmark)
    actual_lines = semantic_lines_from_text(_read_text(actual_path), benchmark)

    if not expected_lines:
        return {
            "status": "unchecked",
            "reference": reference,
            "actual_lines": actual_lines,
            "expected_lines": expected_lines,
            "reason": "reference emitted no semantic lines",
        }

    if actual_lines == expected_lines:
        return {
            "status": "pass",
            "reference": reference,
            "actual_lines": actual_lines,
            "expected_lines": expected_lines,
            "reason": f"matched {len(expected_lines)} semantic line(s)",
        }

    return {
        "status": "fail",
        "reference": reference,
        "actual_lines": actual_lines,
        "expected_lines": expected_lines,
        "reason": _mismatch_reason(expected_lines, actual_lines),
    }


def _write_json(report: dict[str, Any], json_out: str | None) -> None:
    if json_out:
        with open(json_out, "w", encoding="utf-8") as handle:
            json.dump(report, handle, indent=2)
            handle.write("\n")
    else:
        json.dump(report, sys.stdout, indent=2)
        sys.stdout.write("\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", required=True, help="Reference stdout file")
    parser.add_argument("--actual", required=True, help="Perry stdout file")
    parser.add_argument(
        "--reference",
        choices=("node", "none"),
        default="node",
        help="Reference source recorded in the JSON report",
    )
    parser.add_argument(
        "--benchmark",
        required=True,
        help="Suite fixture filename/stem whose declared measurement labels are ignored",
    )
    parser.add_argument("--json-out", help="Write the JSON report to this path")
    args = parser.parse_args(argv)

    if args.reference == "none":
        report = {
            "status": "unchecked",
            "reference": "none",
            "actual_lines": semantic_lines_from_text(_read_text(args.actual), args.benchmark),
            "expected_lines": [],
            "reason": "reference unavailable",
        }
    else:
        report = compare_stdout_files(
            expected_path=args.expected,
            actual_path=args.actual,
            reference=args.reference,
            benchmark=args.benchmark,
        )

    _write_json(report, args.json_out)
    return 1 if report["status"] == "fail" else 0


if __name__ == "__main__":
    raise SystemExit(main())
