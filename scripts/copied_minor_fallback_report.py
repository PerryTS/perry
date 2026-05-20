#!/usr/bin/env python3
"""Summarize copied-minor fallback reasons from PERRY_GC_TRACE output."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 3

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
    "large_excluded_objects",
    "large_excluded_bytes",
    "malloc_registry_rebuilds",
)

LAYOUT_SCAN_TOTALS = (
    "pointer_slots_read",
    "masked_pointer_slots_read",
    "unknown_layout_slots_read",
    "pointer_free_ranges_skipped",
    "pointer_free_slots_skipped",
)

FORBIDDEN_TARGET_MALLOC_KINDS = ("string", "closure")
FORCED_EVACUATION_VERIFIER_WORKLOADS = frozenset(("async_promise_closures",))

DEFAULT_SAFE_FALLBACK_WORKLOADS = (
    "json_roundtrip",
    "string_churn",
    "object_property_churn",
    "mixed_request_shaping",
)
DEFAULT_SAFE_FALLBACK_WORKLOAD_SET = set(DEFAULT_SAFE_FALLBACK_WORKLOADS)


def empty_reason_counts() -> dict[str, int]:
    return {reason: 0 for reason in KNOWN_FALLBACK_REASONS}


def empty_totals() -> dict[str, Any]:
    return {
        "cycles": 0,
        "fallback_reason_counts": empty_reason_counts(),
        "conservative_pinned_bytes": 0,
        "legacy_copy_only_scanner_pinned": {
            "registered_rust_scanners": 0,
            "registered_ffi_scanners": 0,
            "emitted_roots": 0,
            "emitted_young_roots": 0,
            "emitted_old_roots": 0,
            "emitted_malloc_roots": 0,
            "malformed_roots": 0,
            "roots": 0,
            "bytes": 0,
        },
        "copying_nursery": {
            "copied_objects": 0,
            "copied_bytes": 0,
            "promoted_objects": 0,
            "promoted_bytes": 0,
            "large_excluded_objects": 0,
            "large_excluded_bytes": 0,
            "malloc_registry_rebuilds": 0,
            "ineligible_cycles": 0,
        },
        "layout_scans": {field: 0 for field in LAYOUT_SCAN_TOTALS},
        "missing_layout_scans": 0,
        "malloc_kind_allocations": {
            kind: 0 for kind in FORBIDDEN_TARGET_MALLOC_KINDS
        },
        "old_page_accounting": {
            "checked_cycles": 0,
            "dead_bytes": 0,
            "reusable_bytes": 0,
            "returned_bytes": 0,
            "candidate_pages": 0,
            "selected_pages": 0,
            "selected_live_bytes": 0,
            "reclaimable_bytes": 0,
            "old_page_moved_bytes": 0,
            "released_original_bytes": 0,
            "released_original_reusable_bytes": 0,
            "released_original_returned_bytes": 0,
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


def parse_target_malloc_kind_allowance(spec: str) -> tuple[str, str, int]:
    if "=" not in spec:
        raise ValueError(
            "target malloc kind allowance must be WORKLOAD:KIND=COUNT: "
            f"{spec!r}"
        )
    left, count_str = spec.split("=", 1)
    if ":" not in left:
        raise ValueError(
            "target malloc kind allowance must be WORKLOAD:KIND=COUNT: "
            f"{spec!r}"
        )
    workload, kind = left.split(":", 1)
    workload = workload.strip()
    kind = kind.strip()
    count_str = count_str.strip()
    if not workload:
        raise ValueError(f"target malloc kind allowance workload is empty: {spec!r}")
    if kind not in FORBIDDEN_TARGET_MALLOC_KINDS:
        raise ValueError(
            f"target malloc kind allowance kind must be one of "
            f"{', '.join(FORBIDDEN_TARGET_MALLOC_KINDS)}: {spec!r}"
        )
    try:
        count = int(count_str, 10)
    except ValueError as exc:
        raise ValueError(
            f"target malloc kind allowance count must be a non-negative integer: {spec!r}"
        ) from exc
    if count < 0:
        raise ValueError(
            f"target malloc kind allowance count must be a non-negative integer: {spec!r}"
        )
    return workload, kind, count


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
    for field in (
        "registered_rust_scanners",
        "registered_ffi_scanners",
        "emitted_roots",
        "emitted_young_roots",
        "emitted_old_roots",
        "emitted_malloc_roots",
        "malformed_roots",
        "roots",
        "bytes",
    ):
        dst["legacy_copy_only_scanner_pinned"][field] += src[
            "legacy_copy_only_scanner_pinned"
        ][field]
    for field in COPYING_NURSERY_TOTALS:
        dst["copying_nursery"][field] += src["copying_nursery"][field]
    dst["copying_nursery"]["ineligible_cycles"] += src["copying_nursery"][
        "ineligible_cycles"
    ]
    for field in LAYOUT_SCAN_TOTALS:
        dst["layout_scans"][field] += src["layout_scans"][field]
    dst["missing_layout_scans"] += src["missing_layout_scans"]
    for kind in FORBIDDEN_TARGET_MALLOC_KINDS:
        dst["malloc_kind_allocations"][kind] += src["malloc_kind_allocations"][kind]
    for field in (
        "checked_cycles",
        "dead_bytes",
        "reusable_bytes",
        "returned_bytes",
        "candidate_pages",
        "selected_pages",
        "selected_live_bytes",
        "reclaimable_bytes",
        "old_page_moved_bytes",
        "released_original_bytes",
        "released_original_reusable_bytes",
        "released_original_returned_bytes",
    ):
        dst["old_page_accounting"][field] += src["old_page_accounting"][field]


def target_gates_require_copied_minor(name: str) -> bool:
    return (
        not name.startswith("old_page_")
        and name not in FORCED_EVACUATION_VERIFIER_WORKLOADS
    )


def record_malloc_kind_allocations(cycle: dict[str, Any], totals: dict[str, Any]) -> None:
    rows = cycle.get("malloc_kinds")
    if not isinstance(rows, list):
        return
    for row in rows:
        if not isinstance(row, dict):
            continue
        kind = row.get("kind")
        if kind not in FORBIDDEN_TARGET_MALLOC_KINDS:
            continue
        totals["malloc_kind_allocations"][kind] += non_negative_int(
            row, "allocated_count"
        )


def check_old_page_accounting(
    name: str,
    line_number: int,
    cycle: dict[str, Any],
    totals: dict[str, Any],
    errors: list[str],
) -> None:
    old_pages = nested_dict(cycle, "old_pages")
    policy = nested_dict(cycle, "evacuation_policy")
    evacuation = nested_dict(cycle, "evacuation")

    allocated = non_negative_int(old_pages, "allocated_bytes")
    live = non_negative_int(old_pages, "live_bytes")
    dead = non_negative_int(old_pages, "dead_bytes")
    reusable = non_negative_int(old_pages, "reusable_bytes")
    returned = non_negative_int(old_pages, "returned_bytes")
    pinned = non_negative_int(old_pages, "pinned_bytes")
    if allocated > 0:
        totals["old_page_accounting"]["checked_cycles"] += 1
        if live + dead != allocated:
            errors.append(
                f"{name}:{line_number}: old_pages live_bytes({live}) + "
                f"dead_bytes({dead}) != allocated_bytes({allocated})"
            )
        if pinned > live:
            errors.append(
                f"{name}:{line_number}: old_pages pinned_bytes({pinned}) "
                f"> live_bytes({live})"
            )
    totals["old_page_accounting"]["dead_bytes"] += dead
    totals["old_page_accounting"]["reusable_bytes"] += reusable
    totals["old_page_accounting"]["returned_bytes"] += returned

    candidate_pages = non_negative_int(policy, "old_page_candidate_pages")
    selected_pages = non_negative_int(policy, "old_page_selected_pages")
    selected_live = non_negative_int(policy, "old_page_selected_live_bytes")
    reclaimable = non_negative_int(policy, "old_page_reclaimable_bytes")
    old_page_moved = non_negative_int(evacuation, "old_page_moved_bytes")
    released = non_negative_int(evacuation, "released_original_bytes")
    released_reusable = non_negative_int(evacuation, "released_original_reusable_bytes")
    released_returned = non_negative_int(evacuation, "released_original_returned_bytes")

    totals["old_page_accounting"]["candidate_pages"] += candidate_pages
    totals["old_page_accounting"]["selected_pages"] += selected_pages
    totals["old_page_accounting"]["selected_live_bytes"] += selected_live
    totals["old_page_accounting"]["reclaimable_bytes"] += reclaimable
    totals["old_page_accounting"]["old_page_moved_bytes"] += old_page_moved
    totals["old_page_accounting"]["released_original_bytes"] += released
    totals["old_page_accounting"][
        "released_original_reusable_bytes"
    ] += released_reusable
    totals["old_page_accounting"][
        "released_original_returned_bytes"
    ] += released_returned

    if selected_pages > candidate_pages:
        errors.append(
            f"{name}:{line_number}: old_page_selected_pages({selected_pages}) "
            f"> old_page_candidate_pages({candidate_pages})"
        )
    if selected_pages == 0 and (selected_live > 0 or reclaimable > 0):
        errors.append(
            f"{name}:{line_number}: old-page selected bytes reported with no selected pages"
        )
    if old_page_moved > selected_live:
        errors.append(
            f"{name}:{line_number}: evacuation.old_page_moved_bytes({old_page_moved}) "
            f"> evacuation_policy.old_page_selected_live_bytes({selected_live})"
        )
    if old_page_moved > released:
        errors.append(
            f"{name}:{line_number}: evacuation.old_page_moved_bytes({old_page_moved}) "
            f"> evacuation.released_original_bytes({released})"
        )


def aggregate_workload(
    name: str,
    trace_file: Path,
    unknown_reasons: list[dict[str, Any]],
    old_page_errors: list[str],
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
        for field in (
            "registered_rust_scanners",
            "registered_ffi_scanners",
            "emitted_roots",
            "emitted_young_roots",
            "emitted_old_roots",
            "emitted_malloc_roots",
            "malformed_roots",
            "roots",
            "bytes",
        ):
            totals["legacy_copy_only_scanner_pinned"][field] += non_negative_int(
                legacy_pinned, field
            )

        for field in COPYING_NURSERY_TOTALS:
            totals["copying_nursery"][field] += non_negative_int(copying_nursery, field)
        if copying_nursery.get("eligible") is not True:
            totals["copying_nursery"]["ineligible_cycles"] += 1

        layout_scans = nested_dict(cycle, "layout_scans")
        if layout_scans:
            for field in LAYOUT_SCAN_TOTALS:
                totals["layout_scans"][field] += non_negative_int(layout_scans, field)
        else:
            totals["missing_layout_scans"] += 1

        record_malloc_kind_allocations(cycle, totals)
        if name.startswith("old_page_"):
            check_old_page_accounting(name, line_number, cycle, totals, old_page_errors)

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
    parser.add_argument(
        "--target-collector-gates",
        action="store_true",
        help="Fail strict target-collector architecture gates for named trace workloads.",
    )
    parser.add_argument(
        "--strict-fallback-evidence",
        action="store_true",
        help="Fail if default-safe copied-minor fallback evidence uses any fallback path.",
    )
    parser.add_argument(
        "--allow-target-malloc-kind",
        action="append",
        default=[],
        metavar="WORKLOAD:KIND=COUNT",
        help=(
            "Allow up to COUNT allocations of forbidden malloc KIND for one "
            "target-collector workload. KIND is string or closure. May be repeated."
        ),
    )
    return parser


def run_strict_fallback_evidence_gates(
    workloads: dict[str, dict[str, Any]],
    errors: list[str],
) -> None:
    strict_workloads = {
        name: workload
        for name, workload in workloads.items()
        if name in DEFAULT_SAFE_FALLBACK_WORKLOAD_SET
    }
    if not strict_workloads:
        errors.append(
            "strict fallback evidence requires at least one known default-safe workload"
        )

    for name, workload in strict_workloads.items():
        reason_counts = workload["fallback_reason_counts"]
        non_none = {
            reason: count
            for reason, count in reason_counts.items()
            if reason != "none" and count > 0
        }
        if non_none:
            errors.append(f"{name}: fallback reasons other than none: {non_none}")
        if workload["copying_nursery"]["ineligible_cycles"] > 0:
            errors.append(
                f"{name}: copied-minor ineligible cycles="
                f"{workload['copying_nursery']['ineligible_cycles']}"
            )
        if workload["conservative_pinned_bytes"] != 0:
            errors.append(
                f"{name}: conservative_pinned_bytes="
                f"{workload['conservative_pinned_bytes']}, want 0"
            )
        legacy_pinned = workload["legacy_copy_only_scanner_pinned"]["bytes"]
        if legacy_pinned != 0:
            errors.append(
                f"{name}: legacy_copy_only_scanner_pinned.bytes={legacy_pinned}, want 0"
            )
        if workload["copying_nursery"]["malloc_registry_rebuilds"] != 0:
            errors.append(
                f"{name}: malloc_registry_rebuilds="
                f"{workload['copying_nursery']['malloc_registry_rebuilds']}, want 0"
            )


def run_target_collector_gates(
    workloads: dict[str, dict[str, Any]],
    errors: list[str],
    malloc_kind_allowances: dict[str, dict[str, int]] | None = None,
) -> None:
    if malloc_kind_allowances is None:
        malloc_kind_allowances = {}

    strict_workloads = {
        name: workload
        for name, workload in workloads.items()
        if target_gates_require_copied_minor(name)
    }
    if not strict_workloads:
        errors.append("target collector gates require at least one copied-minor workload")

    for name, workload in strict_workloads.items():
        reason_counts = workload["fallback_reason_counts"]
        non_none = {
            reason: count
            for reason, count in reason_counts.items()
            if reason != "none" and count > 0
        }
        if non_none:
            errors.append(f"{name}: fallback reasons other than none: {non_none}")
        if workload["copying_nursery"]["ineligible_cycles"] > 0:
            errors.append(
                f"{name}: copied-minor ineligible cycles="
                f"{workload['copying_nursery']['ineligible_cycles']}"
            )
        if workload["copying_nursery"]["malloc_registry_rebuilds"] != 0:
            errors.append(
                f"{name}: malloc_registry_rebuilds="
                f"{workload['copying_nursery']['malloc_registry_rebuilds']}, want 0"
            )
        if workload["conservative_pinned_bytes"] != 0:
            errors.append(
                f"{name}: conservative_pinned_bytes="
                f"{workload['conservative_pinned_bytes']}, want 0"
            )
        legacy_pinned = workload["legacy_copy_only_scanner_pinned"]["bytes"]
        if legacy_pinned != 0:
            errors.append(
                f"{name}: legacy_copy_only_scanner_pinned.bytes={legacy_pinned}, want 0"
            )
        productive = (
            workload["copying_nursery"]["copied_objects"]
            + workload["copying_nursery"]["promoted_objects"]
        )
        if productive == 0:
            errors.append(f"{name}: no copied-minor cycle copied or promoted an object")
    for name, workload in workloads.items():
        for kind, count in workload["malloc_kind_allocations"].items():
            allowed = malloc_kind_allowances.get(name, {}).get(kind, 0)
            if count > allowed:
                errors.append(
                    f"{name}: forbidden malloc allocation kind {kind} count={count} "
                    f"exceeds allowance={allowed}"
                )

        if workload["missing_layout_scans"] != 0:
            errors.append(
                f"{name}: missing layout_scans on {workload['missing_layout_scans']} cycle(s)"
            )

        if "pointer_free" in name:
            layout = workload["layout_scans"]
            skipped = layout["pointer_free_slots_skipped"]
            read = layout["pointer_slots_read"]
            if skipped == 0:
                errors.append(f"{name}: no pointer-free slots were skipped")
            max_expected_reads = max(8, skipped // 8)
            if read > max_expected_reads:
                errors.append(
                    f"{name}: pointer_slots_read={read} exceeds pointer-free "
                    f"payload allowance {max_expected_reads} for skipped={skipped}"
                )

        if "large" in name and target_gates_require_copied_minor(name):
            copying = workload["copying_nursery"]
            if copying["large_excluded_objects"] == 0 or copying["large_excluded_bytes"] == 0:
                errors.append(f"{name}: missing large-object exclusion telemetry")

        if name.startswith("old_page_"):
            old_page = workload["old_page_accounting"]
            if old_page["selected_pages"] == 0:
                errors.append(f"{name}: forced old-page workload selected no pages")
            if old_page["old_page_moved_bytes"] == 0:
                errors.append(f"{name}: forced old-page workload moved no old-page bytes")


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

    malloc_kind_allowances: dict[str, dict[str, int]] = {}
    for spec in args.allow_target_malloc_kind:
        try:
            workload, kind, count = parse_target_malloc_kind_allowance(spec)
        except ValueError as exc:
            parser.error(str(exc))
        if workload not in workload_names:
            parser.error(
                f"target malloc kind allowance references unknown workload: {workload}"
            )
        existing = malloc_kind_allowances.setdefault(workload, {})
        if kind in existing:
            parser.error(
                f"duplicate target malloc kind allowance for {workload}:{kind}"
            )
        existing[kind] = count

    workloads: dict[str, dict[str, Any]] = {}
    summary = empty_totals()
    summary["workload_count"] = len(parsed_workloads)
    unknown_reasons: list[dict[str, Any]] = []
    old_page_errors: list[str] = []
    errors: list[str] = []

    for name, trace_file in parsed_workloads:
        workload = aggregate_workload(
            name, trace_file, unknown_reasons, old_page_errors, errors
        )
        workloads[name] = workload
        add_totals(summary, workload)

    report = {
        "schema_version": SCHEMA_VERSION,
        "workloads": workloads,
        "summary": summary,
        "unknown_reasons": unknown_reasons,
        "old_page_errors": old_page_errors,
        "top_remaining_reason": top_remaining_reason(summary, workloads),
    }

    if args.target_collector_gates:
        run_target_collector_gates(workloads, errors, malloc_kind_allowances)
        errors.extend(old_page_errors)
    if args.strict_fallback_evidence:
        run_strict_fallback_evidence_gates(workloads, errors)

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
