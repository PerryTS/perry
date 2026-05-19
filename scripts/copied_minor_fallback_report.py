#!/usr/bin/env python3
"""Summarize copied-minor fallback reasons from PERRY_GC_TRACE output."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1

# Keep in sync with CopiedMinorFallbackReason::as_str in
# crates/perry-runtime/src/gc.rs.
KNOWN_FALLBACK_REASONS = (
    "none",
    "copy_only_roots",
    "barriers_inactive",
    "conservative_stack",
    "malloc_registry_unavailable",
    "pinned_young_root",
    "pinned_young_dirty_slot",
    "pinned_young_transitive",
    "not_attempted",
)
KNOWN_FALLBACK_REASON_SET = set(KNOWN_FALLBACK_REASONS)

COPYING_NURSERY_TOTALS = (
    "copied_objects",
    "copied_bytes",
    "promoted_objects",
    "promoted_bytes",
)


def empty_reason_counts() -> dict[str, int]:
    return {reason: 0 for reason in KNOWN_FALLBACK_REASONS}


def empty_totals() -> dict[str, Any]:
    return {
        "cycles": 0,
        "fallback_reason_counts": empty_reason_counts(),
        "conservative_pinned_bytes": 0,
        "legacy_copy_only_scanner_pinned": {
            "roots": 0,
            "bytes": 0,
        },
        "copying_nursery": {
            "copied_objects": 0,
            "copied_bytes": 0,
            "promoted_objects": 0,
            "promoted_bytes": 0,
        },
    }


def parse_workload_spec(spec: str) -> tuple[str, Path]:
    if "=" not in spec:
        raise ValueError(f"workload must be NAME=TRACE_FILE: {spec!r}")
    name, trace_file = spec.split("=", 1)
    name = name.strip()
    if not name:
        raise ValueError(f"workload name is empty: {spec!r}")
    if not trace_file:
        raise ValueError(f"trace file is empty for workload {name!r}")
    return name, Path(trace_file)


def nested_dict(obj: dict[str, Any], *path: str) -> dict[str, Any]:
    cur: Any = obj
    for key in path:
        if not isinstance(cur, dict):
            return {}
        cur = cur.get(key)
    if not isinstance(cur, dict):
        return {}
    return cur


def non_negative_int(obj: dict[str, Any], field: str) -> int:
    value = obj.get(field, 0)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        return 0
    return value


def iter_gc_cycles(trace_file: Path, errors: list[str]):
    try:
        fh = trace_file.open("r", encoding="utf-8", errors="replace")
    except OSError as exc:
        errors.append(f"{trace_file}: cannot read trace file: {exc}")
        return

    with fh:
        for line_number, line in enumerate(fh, start=1):
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict) and event.get("event") == "gc_cycle":
                yield line_number, event


def add_totals(dst: dict[str, Any], src: dict[str, Any]) -> None:
    dst["cycles"] += src["cycles"]
    for reason in KNOWN_FALLBACK_REASONS:
        dst["fallback_reason_counts"][reason] += src["fallback_reason_counts"][reason]
    dst["conservative_pinned_bytes"] += src["conservative_pinned_bytes"]
    dst["legacy_copy_only_scanner_pinned"]["roots"] += src[
        "legacy_copy_only_scanner_pinned"
    ]["roots"]
    dst["legacy_copy_only_scanner_pinned"]["bytes"] += src[
        "legacy_copy_only_scanner_pinned"
    ]["bytes"]
    for field in COPYING_NURSERY_TOTALS:
        dst["copying_nursery"][field] += src["copying_nursery"][field]


def aggregate_workload(
    name: str,
    trace_file: Path,
    unknown_reasons: list[dict[str, Any]],
    errors: list[str],
) -> dict[str, Any]:
    totals = empty_totals()

    for line_number, cycle in iter_gc_cycles(trace_file, errors):
        totals["cycles"] += 1
        copying_nursery = nested_dict(cycle, "copying_nursery")
        fallback_reason = copying_nursery.get("fallback_reason")
        if not isinstance(fallback_reason, str):
            unknown_reasons.append(
                {
                    "workload": name,
                    "line": line_number,
                    "reason": fallback_reason,
                    "error": "copying_nursery.fallback_reason is missing or not a string",
                }
            )
        elif fallback_reason not in KNOWN_FALLBACK_REASON_SET:
            unknown_reasons.append(
                {
                    "workload": name,
                    "line": line_number,
                    "reason": fallback_reason,
                    "error": "unknown copying_nursery.fallback_reason",
                }
            )
        else:
            totals["fallback_reason_counts"][fallback_reason] += 1

        totals["conservative_pinned_bytes"] += non_negative_int(
            cycle, "conservative_pinned_bytes"
        )

        legacy_pinned = nested_dict(cycle, "legacy_copy_only_scanner_pinned")
        totals["legacy_copy_only_scanner_pinned"]["roots"] += non_negative_int(
            legacy_pinned, "roots"
        )
        totals["legacy_copy_only_scanner_pinned"]["bytes"] += non_negative_int(
            legacy_pinned, "bytes"
        )

        for field in COPYING_NURSERY_TOTALS:
            totals["copying_nursery"][field] += non_negative_int(copying_nursery, field)

    if totals["cycles"] == 0:
        errors.append(f"{name}: no gc_cycle JSON events found in {trace_file}")

    return totals


def top_remaining_reason(
    summary: dict[str, Any],
    workloads: dict[str, dict[str, Any]],
) -> dict[str, Any] | None:
    reason_counts = summary["fallback_reason_counts"]
    candidates = [
        (reason, reason_counts[reason])
        for reason in KNOWN_FALLBACK_REASONS
        if reason != "none" and reason_counts[reason] > 0
    ]
    if not candidates:
        return None

    reason, count = sorted(candidates, key=lambda item: (-item[1], item[0]))[0]
    workload_counts = {
        name: workload["fallback_reason_counts"][reason]
        for name, workload in workloads.items()
        if workload["fallback_reason_counts"][reason] > 0
    }
    return {
        "reason": reason,
        "count": count,
        "workloads": workload_counts,
    }


def write_report(report: dict[str, Any], out: str | None) -> None:
    if out:
        with Path(out).open("w", encoding="utf-8") as fh:
            json.dump(report, fh, indent=2)
            fh.write("\n")
    else:
        json.dump(report, sys.stdout, indent=2)
        sys.stdout.write("\n")


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Summarize copied-minor fallback reasons from gc_cycle trace JSON."
    )
    parser.add_argument(
        "--workload",
        action="append",
        default=[],
        metavar="NAME=TRACE_FILE",
        help="Named PERRY_GC_TRACE stderr file to include. May be repeated.",
    )
    parser.add_argument(
        "--out",
        help="Write report JSON to this path. Defaults to stdout.",
    )
    return parser


def main(argv: list[str]) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)

    if not args.workload:
        parser.error("at least one --workload NAME=TRACE_FILE is required")

    parsed_workloads: list[tuple[str, Path]] = []
    workload_names: set[str] = set()
    for spec in args.workload:
        try:
            name, trace_file = parse_workload_spec(spec)
        except ValueError as exc:
            parser.error(str(exc))
        if name in workload_names:
            parser.error(f"duplicate workload name: {name}")
        workload_names.add(name)
        parsed_workloads.append((name, trace_file))

    workloads: dict[str, dict[str, Any]] = {}
    summary = empty_totals()
    summary["workload_count"] = len(parsed_workloads)
    unknown_reasons: list[dict[str, Any]] = []
    errors: list[str] = []

    for name, trace_file in parsed_workloads:
        workload = aggregate_workload(name, trace_file, unknown_reasons, errors)
        workloads[name] = workload
        add_totals(summary, workload)

    report = {
        "schema_version": SCHEMA_VERSION,
        "workloads": workloads,
        "summary": summary,
        "unknown_reasons": unknown_reasons,
        "top_remaining_reason": top_remaining_reason(summary, workloads),
    }

    write_report(report, args.out)

    if unknown_reasons:
        errors.append(f"found {len(unknown_reasons)} unknown or malformed fallback reason(s)")
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
