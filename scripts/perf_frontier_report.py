#!/usr/bin/env python3
"""Build and gate Perry performance-frontier evidence packets."""

from __future__ import annotations

import argparse
import json
import math
import re
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
EXACT_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
FULL_CHECKSUM_REL_TOLERANCE = 5e-6
GC_BOUND_MIN_SHARE = 0.10
GC_BOUND_MIN_PAUSE_MS = 20.0
TRIGGER_POLICY_MIN_CYCLES = 3
TRIGGER_POLICY_MAX_RECLAIM_RATIO = 0.10

CLASS_GC_BOUND = "GC-bound"
CLASS_TRIGGER_POLICY_BOUND = "trigger-policy-bound"
CLASS_PROPERTY_METHOD_DISPATCH_BOUND = "property/method-dispatch-bound"
CLASS_NUMERIC_REPRESENTATION_BOUND = "numeric-representation-bound"
CLASS_BOUNDS_CHECK_LOOP_BOUND = "bounds-check/loop-bound"
CLASS_HELPER_RUNTIME_CALL_BOUND = "helper/runtime-call-bound"
CLASS_EXTERNAL_NATIVE_BOUND = "external/native-bound"

ALLOWED_CLASSIFICATIONS = frozenset(
    {
        CLASS_GC_BOUND,
        CLASS_TRIGGER_POLICY_BOUND,
        CLASS_PROPERTY_METHOD_DISPATCH_BOUND,
        CLASS_NUMERIC_REPRESENTATION_BOUND,
        CLASS_BOUNDS_CHECK_LOOP_BOUND,
        CLASS_HELPER_RUNTIME_CALL_BOUND,
        CLASS_EXTERNAL_NATIVE_BOUND,
    }
)

REQUIRED_BENCHMARK_ROWS = (
    "bench_json_roundtrip",
    "bench_gc_pressure",
    "07_object_create",
    "12_binary_trees",
)

DEFAULT_TRACE_ROWS = REQUIRED_BENCHMARK_ROWS

GC_SYMBOL_HINTS = (
    "gc_",
    "collect",
    "sweep",
    "mark",
    "evac",
    "nursery",
    "remembered",
)


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def load_json(path: Path, default: Any = None) -> Any:
    if not path.exists():
        return default
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def nested(obj: Any, *path: str, default: Any = None) -> Any:
    cur = obj
    for key in path:
        if not isinstance(cur, dict):
            return default
        cur = cur.get(key, default)
    return cur


def as_int(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def as_float(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    if isinstance(value, str):
        try:
            parsed = float(value)
        except ValueError:
            return None
        if math.isfinite(parsed):
            return parsed
    return None


def exact_sha(value: Any) -> bool:
    return isinstance(value, str) and EXACT_SHA_RE.fullmatch(value) is not None


def rel_delta(a: float | None, b: float | None) -> float | None:
    if a is None or b is None:
        return None
    denom = max(abs(a), abs(b), 1.0)
    return abs(a - b) / denom


def checksum_within_tolerance(
    node_checksum: float | None,
    perry_checksum: float | None,
    *,
    tolerance: float = FULL_CHECKSUM_REL_TOLERANCE,
) -> bool:
    delta = rel_delta(node_checksum, perry_checksum)
    return delta is not None and delta <= tolerance


def add_counter_fields(counter: Counter[str], data: Any, prefix: str = "") -> None:
    if not isinstance(data, dict):
        return
    for key, value in data.items():
        name = f"{prefix}.{key}" if prefix else str(key)
        if isinstance(value, bool):
            continue
        if isinstance(value, int):
            counter[name] += max(value, 0)


def iter_gc_cycles(trace_path: Path) -> tuple[list[dict[str, Any]], int]:
    events: list[dict[str, Any]] = []
    malformed = 0
    if not trace_path.exists():
        return events, malformed
    with trace_path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                malformed += 1
                continue
            if isinstance(event, dict) and event.get("event") == "gc_cycle":
                events.append(event)
    return events, malformed


def summarize_gc_trace(
    trace_path: Path,
    *,
    workload: str,
    stdout_path: Path | None = None,
    out_trace_path: Path | None = None,
) -> dict[str, Any]:
    events, malformed = iter_gc_cycles(trace_path)
    phase_us: Counter[str] = Counter()
    trigger_kinds: Counter[str] = Counter()
    collection_kinds: Counter[str] = Counter()
    fallback_reasons: Counter[str] = Counter()
    roots: Counter[str] = Counter()
    remembered_set: Counter[str] = Counter()
    layout_scans: Counter[str] = Counter()
    copying_nursery: Counter[str] = Counter()
    malloc_objects: Counter[str] = Counter()
    malloc_kinds: Counter[str] = Counter()
    old_pages: Counter[str] = Counter()
    evacuation: Counter[str] = Counter()
    evacuation_policy: Counter[str] = Counter()
    sweep: Counter[str] = Counter()
    steps: dict[str, dict[str, Any]] = {}
    pause_us_total = 0
    pause_us_max = 0
    before_in_use_total = 0
    after_in_use_total = 0

    for event in events:
        pause = as_int(event.get("pause_us"))
        pause_us_total += pause
        pause_us_max = max(pause_us_max, pause)
        phase = event.get("phase_us", {})
        if isinstance(phase, dict):
            for name, value in phase.items():
                phase_us[str(name)] += as_int(value)

        trigger_kind = nested(event, "trigger", "kind")
        trigger_kinds[str(trigger_kind) if isinstance(trigger_kind, str) else "_missing"] += 1
        collection_kind = event.get("collection_kind")
        collection_kinds[
            str(collection_kind) if isinstance(collection_kind, str) else "_missing"
        ] += 1
        fallback = nested(event, "copying_nursery", "fallback_reason")
        fallback_reasons[str(fallback) if isinstance(fallback, str) else "_missing"] += 1

        roots["conservative_root_count"] += as_int(event.get("conservative_root_count"))
        roots["conservative_pinned"] += as_int(event.get("conservative_pinned"))
        roots["conservative_pinned_bytes"] += as_int(event.get("conservative_pinned_bytes"))
        roots["compiled_frame_conservative_pinned_bytes"] += as_int(
            event.get("compiled_frame_conservative_pinned_bytes")
        )
        roots["runtime_conservative_pinned_bytes"] += as_int(
            event.get("runtime_conservative_pinned_bytes")
        )
        roots["conservative_stack_scan_bytes"] += as_int(
            event.get("conservative_stack_scan_bytes")
        )
        roots["conservative_stack_truncated"] += (
            1 if event.get("conservative_stack_truncated") is True else 0
        )
        roots["conservative_stack_unbounded"] += (
            1 if event.get("conservative_stack_unbounded") is True else 0
        )
        add_counter_fields(roots, event.get("legacy_copy_only_scanner_pinned"), "legacy")
        add_counter_fields(roots, event.get("shadow_roots"), "shadow")

        add_counter_fields(remembered_set, event.get("remembered_set"))
        add_counter_fields(layout_scans, event.get("layout_scans"))
        add_counter_fields(copying_nursery, event.get("copying_nursery"))
        add_counter_fields(malloc_objects, event.get("malloc_objects"))
        add_counter_fields(old_pages, event.get("old_pages"))
        add_counter_fields(evacuation, event.get("evacuation"))
        add_counter_fields(evacuation_policy, event.get("evacuation_policy"))
        add_counter_fields(sweep, event.get("sweep"))

        for kind in event.get("malloc_kinds", []) if isinstance(event.get("malloc_kinds"), list) else []:
            if not isinstance(kind, dict):
                continue
            name = kind.get("kind")
            prefix = f"{name}" if isinstance(name, str) else "unknown"
            add_counter_fields(malloc_kinds, kind, prefix)

        step_snapshot = event.get("steps", {})
        if isinstance(step_snapshot, dict):
            for name, value in step_snapshot.items():
                if not isinstance(value, dict):
                    continue
                entry = steps.setdefault(
                    str(name),
                    {
                        "before_min": None,
                        "before_max": None,
                        "after_min": None,
                        "after_max": None,
                        "true_before": 0,
                        "true_after": 0,
                    },
                )
                for side in ("before", "after"):
                    step_value = value.get(side)
                    if isinstance(step_value, bool):
                        if step_value:
                            entry[f"true_{side}"] += 1
                        continue
                    if not isinstance(step_value, int):
                        continue
                    min_key = f"{side}_min"
                    max_key = f"{side}_max"
                    entry[min_key] = (
                        step_value if entry[min_key] is None else min(entry[min_key], step_value)
                    )
                    entry[max_key] = (
                        step_value if entry[max_key] is None else max(entry[max_key], step_value)
                    )

        before_in_use_total += as_int(
            nested(event, "arena_bytes", "before", "total_in_use_bytes", default=0)
        )
        after_in_use_total += as_int(
            nested(event, "arena_bytes", "after", "total_in_use_bytes", default=0)
        )

    copied_bytes = copying_nursery.get("copied_bytes", 0)
    promoted_bytes = copying_nursery.get("promoted_bytes", 0)
    moved_bytes = evacuation.get("moved_bytes", 0)
    old_page_moved_bytes = evacuation.get("old_page_moved_bytes", 0)
    freed_bytes = evacuation.get("released_original_bytes", 0) + sweep.get("freed_bytes", 0)
    productive_reclaim_bytes = max(before_in_use_total - after_in_use_total, 0)

    return {
        "schema_version": SCHEMA_VERSION,
        "workload": workload,
        "trace_path": str(out_trace_path or trace_path),
        "stdout_path": str(stdout_path) if stdout_path else None,
        "present": trace_path.exists(),
        "malformed_json_lines": malformed,
        "gc_cycle_count": len(events),
        "pause_us": {
            "total": pause_us_total,
            "max": pause_us_max,
            "avg": round(pause_us_total / len(events), 1) if events else 0,
        },
        "phase_us": dict(sorted(phase_us.items())),
        "trigger_kind_counts": dict(sorted(trigger_kinds.items())),
        "collection_kind_counts": dict(sorted(collection_kinds.items())),
        "fallback_reason_counts": dict(sorted(fallback_reasons.items())),
        "roots": dict(sorted(roots.items())),
        "remembered_set": dict(sorted(remembered_set.items())),
        "layout_scans": dict(sorted(layout_scans.items())),
        "malloc_objects": dict(sorted(malloc_objects.items())),
        "malloc_kinds": dict(sorted(malloc_kinds.items())),
        "copying_nursery": dict(sorted(copying_nursery.items())),
        "evacuation": dict(sorted(evacuation.items())),
        "evacuation_policy": dict(sorted(evacuation_policy.items())),
        "sweep": dict(sorted(sweep.items())),
        "old_pages": dict(sorted(old_pages.items())),
        "steps": steps,
        "byte_totals": {
            "copied_bytes": copied_bytes,
            "promoted_bytes": promoted_bytes,
            "moved_bytes": moved_bytes,
            "old_page_moved_bytes": old_page_moved_bytes,
            "freed_bytes": freed_bytes,
            "productive_reclaim_bytes": productive_reclaim_bytes,
        },
    }


def parse_key_value_output(path: Path) -> dict[str, Any]:
    data: dict[str, Any] = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        parsed_float = as_float(value)
        if parsed_float is not None:
            data[key] = parsed_float
        else:
            data[key] = value
    return data


def parse_duration_list(value: Any) -> list[float]:
    if not isinstance(value, str):
        return []
    result: list[float] = []
    for part in value.split(","):
        parsed = as_float(part.strip())
        if parsed is not None:
            result.append(parsed)
    return result


def math_benchmark_json(root: Path, label: str, out_dir: Path) -> dict[str, Any]:
    node = parse_key_value_output(out_dir / "node.out")
    perry = parse_key_value_output(out_dir / "perry.out")
    compile_text = (out_dir / "compile.out").read_text(
        encoding="utf-8", errors="replace"
    ) if (out_dir / "compile.out").exists() else ""
    node_checksum = as_float(node.get("checksum"))
    perry_checksum = as_float(perry.get("checksum"))
    node_ms = as_float(node.get("medianMs"))
    perry_ms = as_float(perry.get("medianMs"))
    checksum_delta = rel_delta(node_checksum, perry_checksum)
    return {
        "schema_version": SCHEMA_VERSION,
        "label": label,
        "path": str(out_dir),
        "source": str(root / "tmp" / "benchmark-math" / "benchmark.ts"),
        "node": {
            "median_ms": node_ms,
            "checksum": node_checksum,
            "durations_ms": parse_duration_list(node.get("durationsMs")),
        },
        "perry": {
            "median_ms": perry_ms,
            "checksum": perry_checksum,
            "durations_ms": parse_duration_list(perry.get("durationsMs")),
        },
        "perry_to_node_ratio": (
            round(perry_ms / node_ms, 3)
            if node_ms is not None and perry_ms is not None and node_ms > 0
            else None
        ),
        "checksum_relative_delta": checksum_delta,
        "checksum_tolerance": FULL_CHECKSUM_REL_TOLERANCE,
        "checksum_gate": (
            "pass"
            if checksum_within_tolerance(node_checksum, perry_checksum)
            else "fail"
        ),
        "compile_mode_observed": (
            "native"
            if "1 native" in compile_text
            else "fallback" if "JavaScript" in compile_text else "unknown"
        ),
    }


def discover_slice_source(root: Path, bench_name: str) -> Path | None:
    if bench_name == "benchmark":
        return root / "tmp" / "benchmark-math" / "benchmark.ts"
    slices = root / "tmp" / "benchmark-math" / "slices"
    if not slices.exists():
        return None
    for path in slices.glob("*.ts"):
        if path.stem.endswith(bench_name):
            return path
    return None


def slice_results_json(root: Path, label: str, out_dir: Path) -> dict[str, Any]:
    runs = out_dir / "slice-out" / "runs"
    entries: list[dict[str, Any]] = []
    if runs.exists():
        for node_out in sorted(runs.glob("*.node.out")):
            stem = node_out.name[: -len(".node.out")]
            perry_out = runs / f"{stem}.perry.out"
            if not perry_out.exists():
                continue
            node = parse_key_value_output(node_out)
            perry = parse_key_value_output(perry_out)
            bench = str(node.get("bench") or perry.get("bench") or stem)
            node_ms = as_float(node.get("medianMs"))
            perry_ms = as_float(perry.get("medianMs"))
            node_checksum = as_float(node.get("checksum"))
            perry_checksum = as_float(perry.get("checksum"))
            entries.append(
                {
                    "name": bench,
                    "stem": stem,
                    "source": str(discover_slice_source(root, bench) or ""),
                    "node_ms": node_ms,
                    "perry_ms": perry_ms,
                    "perry_to_node_ratio": (
                        round(perry_ms / node_ms, 3)
                        if node_ms is not None and perry_ms is not None and node_ms > 0
                        else None
                    ),
                    "node_checksum": node_checksum,
                    "perry_checksum": perry_checksum,
                    "checksum_relative_delta": rel_delta(node_checksum, perry_checksum),
                }
            )
    return {
        "schema_version": SCHEMA_VERSION,
        "label": label,
        "path": str(out_dir),
        "rows": entries,
    }


def render_math_results(math: dict[str, Any], slices: dict[str, Any], generated_at: str) -> str:
    node_ms = nested(math, "node", "median_ms")
    perry_ms = nested(math, "perry", "median_ms")
    ratio = math.get("perry_to_node_ratio")
    checksum_delta = math.get("checksum_relative_delta")
    verdict = "passes" if math.get("checksum_gate") == "pass" else "fails"
    lines = [
        "# Three.js Math Benchmark Results",
        "",
        f"Generated: `{generated_at}`",
        "",
        "## Full Benchmark",
        "",
        f"- Source: `{math.get('source', 'tmp/benchmark-math/benchmark.ts')}`",
        f"- Node median: `{node_ms}` ms",
        f"- Perry median: `{perry_ms}` ms",
        f"- Perry / Node time ratio: `{ratio}`",
        f"- Checksum relative delta: `{checksum_delta}`",
        f"- Checksum gate: `{verdict}` at relative error `<= {FULL_CHECKSUM_REL_TOLERANCE:g}`",
        f"- Perry compile mode observed: `{math.get('compile_mode_observed', 'unknown')}`",
        "",
        "## Slice Rows",
        "",
        "| Benchmark | Node ms | Perry ms | Perry / Node | Checksum relative delta |",
        "|---|---:|---:|---:|---:|",
    ]
    for row in slices.get("rows", []):
        lines.append(
            f"| `{row.get('name')}` | {row.get('node_ms')} | {row.get('perry_ms')} "
            f"| {row.get('perry_to_node_ratio')} | {row.get('checksum_relative_delta')} |"
        )
    lines.extend(
        [
            "",
            "Raw logs are captured in the perf-frontier packet and in `tmp/benchmark-math`.",
            "",
        ]
    )
    return "\n".join(lines)


def parse_profile_text(text: str) -> list[dict[str, Any]]:
    counts: Counter[str] = Counter()
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        perf_match = re.match(r"^\s*([\d.]+)%\s+.+?\s+(\S[^[]+)$", line)
        if perf_match:
            percent = as_float(perf_match.group(1)) or 0.0
            symbol = perf_match.group(2).strip()
            counts[symbol] += max(int(percent * 1000), 1)
            continue
        sample_match = re.match(r"^\s*(\d+)\s+(.+)$", line)
        if not sample_match:
            continue
        count = int(sample_match.group(1))
        symbol = sample_match.group(2).strip()
        if (
            symbol.startswith("Thread ")
            or symbol.startswith("Thread_")
            or symbol.startswith("DispatchQueue ")
            or "DispatchQueue_" in symbol
            or symbol.startswith("start ")
            or "samples" in symbol.lower()
            or symbol.startswith("0x")
        ):
            continue
        symbol = re.sub(r"\s+\+?\d+(?:\.\d+)?%.*$", "", symbol).strip()
        symbol = re.sub(r"\s+\(in .*\)$", "", symbol).strip()
        if symbol:
            counts[symbol] += count
    total = sum(counts.values()) or 1
    rows = []
    for symbol, count in counts.most_common(20):
        rows.append(
            {
                "symbol": symbol,
                "samples": count,
                "sample_share": round(count / total, 4),
                "is_gc": any(hint in symbol.lower() for hint in GC_SYMBOL_HINTS),
            }
        )
    return rows


def profile_summary(
    *,
    raw_path: Path,
    row_name: str,
    tool: str,
    source: str,
    status: str = "pass",
    reason: str = "",
) -> dict[str, Any]:
    text = raw_path.read_text(encoding="utf-8", errors="replace") if raw_path.exists() else ""
    rows = parse_profile_text(text)
    non_gc = [row for row in rows if not row["is_gc"]]
    return {
        "schema_version": SCHEMA_VERSION,
        "status": status if raw_path.exists() and non_gc else "fail",
        "reason": reason if raw_path.exists() else reason or "raw profiler output missing",
        "requested": True,
        "row": row_name,
        "source": source,
        "tool": tool,
        "raw_path": str(raw_path),
        "top_costs": rows[:10],
        "top_non_gc_costs": non_gc[:3],
    }


def command_status(metadata: dict[str, Any], label: str, command: str) -> str:
    status = nested(metadata, "commands", label, command, "status")
    return status if isinstance(status, str) else "missing"


def command_exit(metadata: dict[str, Any], label: str, command: str) -> int | None:
    value = nested(metadata, "commands", label, command, "exit_code")
    return value if isinstance(value, int) else None


def label_paths(root: Path, label: str) -> dict[str, Path]:
    base = root / label
    return {
        "benchmarks": base / "benchmarks" / "full.json",
        "memory_summary": base / "memory" / "reports" / "memory_stability_summary.json",
        "copied_minor": base / "memory" / "reports" / "copied_minor_fallback_report.json",
        "math": base / "benchmark-math" / "math-benchmark.json",
        "slices": base / "benchmark-math" / "slice-results.json",
        "trace_summaries": base / "direct-traces" / "summaries",
    }


def load_trace_summaries(root: Path, label: str) -> dict[str, Any]:
    summaries: dict[str, Any] = {}
    summary_root = label_paths(root, label)["trace_summaries"]
    if not summary_root.exists():
        return summaries
    for path in sorted(summary_root.glob("*.json")):
        data = load_json(path, {})
        workload = data.get("workload") if isinstance(data, dict) else None
        if isinstance(workload, str):
            summaries[workload] = data
    return summaries


def benchmark_rows(report: dict[str, Any]) -> dict[str, Any]:
    rows = report.get("benchmarks", {}) if isinstance(report, dict) else {}
    return rows if isinstance(rows, dict) else {}


def gc_pause_share(trace: dict[str, Any], perry_ms: float | None) -> float | None:
    if perry_ms is None or perry_ms <= 0:
        return None
    pause_ms = as_int(nested(trace, "pause_us", "total", default=0)) / 1000.0
    return pause_ms / perry_ms


def productive_reclaim_ratio(trace: dict[str, Any]) -> float | None:
    byte_totals = trace.get("byte_totals", {}) if isinstance(trace, dict) else {}
    reclaimed = as_int(byte_totals.get("productive_reclaim_bytes"))
    copied = as_int(byte_totals.get("copied_bytes")) + as_int(byte_totals.get("promoted_bytes"))
    moved = as_int(byte_totals.get("moved_bytes")) + as_int(byte_totals.get("old_page_moved_bytes"))
    total_motion = copied + moved + reclaimed
    if total_motion <= 0:
        return 0.0 if as_int(trace.get("gc_cycle_count")) else None
    return reclaimed / total_motion


def classify_row(
    name: str,
    entry: dict[str, Any],
    trace: dict[str, Any] | None,
    profile: dict[str, Any] | None = None,
) -> dict[str, Any]:
    perry_ms = as_float(entry.get("perry_ms"))
    reasons: list[str] = []
    evidence: dict[str, Any] = {}

    if trace:
        pause_ms = as_int(nested(trace, "pause_us", "total", default=0)) / 1000.0
        share = gc_pause_share(trace, perry_ms)
        cycles = as_int(trace.get("gc_cycle_count"))
        reclaim_ratio = productive_reclaim_ratio(trace)
        trigger_counts = trace.get("trigger_kind_counts", {})
        evidence.update(
            {
                "gc_cycle_count": cycles,
                "gc_pause_ms": round(pause_ms, 3),
                "gc_pause_share": None if share is None else round(share, 4),
                "productive_reclaim_ratio": (
                    None if reclaim_ratio is None else round(reclaim_ratio, 4)
                ),
                "trigger_kind_counts": trigger_counts,
            }
        )
        if (
            share is not None
            and share >= GC_BOUND_MIN_SHARE
            and pause_ms >= GC_BOUND_MIN_PAUSE_MS
        ):
            return {
                "class": CLASS_GC_BOUND,
                "reasons": [
                    f"GC pause is {share:.1%} of Perry wall time ({pause_ms:.1f}ms)"
                ],
                "evidence": evidence,
            }
        pressure_triggers = 0
        if isinstance(trigger_counts, dict):
            pressure_triggers = sum(
                as_int(trigger_counts.get(kind))
                for kind in ("arena_bytes", "malloc_count", "old_gen_bytes")
            )
        if (
            cycles >= TRIGGER_POLICY_MIN_CYCLES
            and pressure_triggers >= max(2, cycles // 2)
            and reclaim_ratio is not None
            and reclaim_ratio <= TRIGGER_POLICY_MAX_RECLAIM_RATIO
        ):
            return {
                "class": CLASS_TRIGGER_POLICY_BOUND,
                "reasons": [
                    "repeated pressure-trigger GC cycles had low productive reclaim"
                ],
                "evidence": evidence,
            }

    lowered = name.lower()
    profile_symbols = []
    if profile and isinstance(profile.get("top_non_gc_costs"), list):
        profile_symbols = [
            str(row.get("symbol", "")).lower()
            for row in profile["top_non_gc_costs"]
            if isinstance(row, dict)
        ]
    symbol_text = " ".join(profile_symbols)

    if "object" in lowered or "property" in lowered or "method" in lowered:
        classification = CLASS_PROPERTY_METHOD_DISPATCH_BOUND
        reasons.append("row shape stresses object property or method dispatch")
    elif "json" in lowered or "string" in lowered or "buffer" in lowered:
        classification = CLASS_HELPER_RUNTIME_CALL_BOUND
        reasons.append("row depends heavily on runtime helpers or native library paths")
    elif "array" in lowered or "matrix" in lowered or "binary_trees" in lowered:
        classification = CLASS_BOUNDS_CHECK_LOOP_BOUND
        reasons.append("row stresses indexed data access and loop codegen")
    elif "math" in lowered or "fibonacci" in lowered or "prime" in lowered:
        classification = CLASS_NUMERIC_REPRESENTATION_BOUND
        reasons.append("row is dominated by numeric operations")
    elif "js_object_get" in symbol_text or "own_field" in symbol_text:
        classification = CLASS_PROPERTY_METHOD_DISPATCH_BOUND
        reasons.append("profiler points at object field lookup")
    elif "array" in symbol_text or "bounds" in symbol_text:
        classification = CLASS_BOUNDS_CHECK_LOOP_BOUND
        reasons.append("profiler points at array or bounds helpers")
    elif "extern" in symbol_text or "native" in symbol_text:
        classification = CLASS_EXTERNAL_NATIVE_BOUND
        reasons.append("profiler points at external/native fallback work")
    else:
        classification = CLASS_NUMERIC_REPRESENTATION_BOUND
        reasons.append("no material GC signal; remaining evidence points at typed CPU work")

    if profile_symbols:
        evidence["profile_top_non_gc_symbols"] = profile_symbols[:3]

    return {
        "class": classification,
        "reasons": reasons,
        "evidence": evidence,
    }


def build_classifications(root: Path, label: str, profile: dict[str, Any] | None) -> dict[str, Any]:
    bench_report = load_json(label_paths(root, label)["benchmarks"], {})
    traces = load_trace_summaries(root, label)
    classifications: dict[str, Any] = {}
    for name, entry in benchmark_rows(bench_report).items():
        if not isinstance(entry, dict):
            continue
        trace = traces.get(name)
        classifications[name] = classify_row(name, entry, trace, profile)

    math = load_json(label_paths(root, label)["math"], {})
    if isinstance(math, dict) and math:
        entry = {
            "perry_ms": nested(math, "perry", "median_ms"),
            "node_ms": nested(math, "node", "median_ms"),
        }
        classifications["benchmark-math/full"] = classify_row(
            "benchmark-math/full", entry, None, profile
        )
    slices = load_json(label_paths(root, label)["slices"], {})
    for row in slices.get("rows", []) if isinstance(slices, dict) else []:
        if not isinstance(row, dict):
            continue
        name = f"benchmark-math/{row.get('name')}"
        entry = {"perry_ms": row.get("perry_ms"), "node_ms": row.get("node_ms")}
        classifications[name] = classify_row(name, entry, None, profile)
    return classifications


def classification_taxonomy_errors(classifications: Any) -> list[str]:
    if not isinstance(classifications, dict):
        return ["classification is not an object"]
    errors: list[str] = []
    for name, entry in sorted(classifications.items()):
        if not isinstance(entry, dict):
            errors.append(f"{name}: classification entry is not an object")
            continue
        value = entry.get("class")
        if value not in ALLOWED_CLASSIFICATIONS:
            errors.append(
                f"{name}: classification class {value!r} is not in allowed taxonomy"
            )
    return errors


def baseline_reference(
    baseline_in: Path | None,
    errors: list[str],
    warnings: list[str],
    *,
    gate: bool,
) -> dict[str, Any]:
    if baseline_in is None:
        return {}

    input_path = baseline_in
    path = baseline_in.expanduser()
    reference: dict[str, Any] = {
        "input_path": str(input_path),
        "resolved_path": str(path.resolve()),
        "present": path.exists(),
    }
    if not path.exists():
        (errors if gate else warnings).append(
            f"baseline reference is missing: {baseline_in}"
        )
        return reference

    data = load_json(path, {})
    if not isinstance(data, dict):
        (errors if gate else warnings).append(
            f"baseline reference is not a JSON object: {baseline_in}"
        )
        return reference

    reference.update(
        {
            "schema_version": data.get("schema_version"),
            "generated_at": data.get("generated_at"),
            "baseline_sha": data.get("baseline_sha"),
            "comparison_base_sha": data.get("comparison_base_sha"),
            "selected_rows": data.get("selected_rows", []),
        }
    )
    if not exact_sha(data.get("baseline_sha")):
        (errors if gate else warnings).append(
            f"baseline reference baseline_sha is not an exact 40-char SHA: {baseline_in}"
        )
    return reference


def collect_report(
    root: Path,
    *,
    gate: bool = False,
    baseline_in: Path | None = None,
) -> dict[str, Any]:
    metadata = load_json(root / "metadata.json", {})
    profile = load_json(root / "profile_summary.json", {})
    errors: list[str] = []
    warnings: list[str] = []
    baseline = baseline_reference(baseline_in, errors, warnings, gate=gate)

    for key in ("base_sha", "head_sha"):
        if not exact_sha(metadata.get(key)):
            (errors if gate else warnings).append(f"metadata {key} is not an exact 40-char SHA")

    for label in ("base", "head"):
        for command in ("build", "benchmarks", "direct_traces", "benchmark_math"):
            status = command_status(metadata, label, command)
            if status != "pass":
                (errors if gate else warnings).append(f"{label}:{command} status is {status}")
        memory_status = command_status(metadata, label, "memory_stability")
        if memory_status != "pass":
            (errors if gate else warnings).append(
                f"{label}:memory_stability status is {memory_status}"
            )
        summary = load_json(label_paths(root, label)["memory_summary"], {})
        if not summary:
            (errors if gate else warnings).append(f"{label}: memory summary missing")
        elif as_int(summary.get("failed")):
            errors.append(f"{label}: memory stability failed={as_int(summary.get('failed'))}")

        bench = load_json(label_paths(root, label)["benchmarks"], {})
        if not bench:
            (errors if gate else warnings).append(f"{label}: benchmark JSON missing")
        for name, entry in benchmark_rows(bench).items():
            if not isinstance(entry, dict):
                continue
            correctness = entry.get("correctness")
            if not isinstance(correctness, dict):
                (errors if gate else warnings).append(
                    f"{label}:{name}: correctness output missing"
                )
                continue
            status = correctness.get("status")
            if status != "pass":
                errors.append(f"{label}:{name}: correctness status is {status}")

        traces = load_trace_summaries(root, label)
        requested_trace_rows = metadata.get("trace_rows")
        if not isinstance(requested_trace_rows, list) or not requested_trace_rows:
            requested_trace_rows = list(DEFAULT_TRACE_ROWS)
        for row in requested_trace_rows:
            if row not in traces:
                (errors if gate else warnings).append(f"{label}:{row}: trace summary missing")

    head_math = load_json(label_paths(root, "head")["math"], {})
    if not head_math:
        (errors if gate else warnings).append("head: benchmark-math full JSON missing")
    elif head_math.get("checksum_gate") != "pass":
        errors.append(
            "head: benchmark-math checksum relative delta "
            f"{head_math.get('checksum_relative_delta')} exceeds "
            f"{FULL_CHECKSUM_REL_TOLERANCE:g}"
        )

    if gate:
        if not isinstance(profile, dict) or not profile:
            errors.append("profile_summary.json is missing")
        elif profile.get("requested") and not profile.get("top_non_gc_costs"):
            errors.append("requested typed-row profiler attribution is missing")
        elif profile.get("status") != "pass":
            errors.append(f"profile_summary status is {profile.get('status')}")

    classifications = build_classifications(root, "head", profile if isinstance(profile, dict) else None)
    for row in REQUIRED_BENCHMARK_ROWS:
        if row not in classifications:
            (errors if gate else warnings).append(f"{row}: classification missing")
    taxonomy_errors = classification_taxonomy_errors(classifications)
    (errors if gate else warnings).extend(taxonomy_errors)

    packet = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": utc_now(),
        "status": "fail" if errors else "pass",
        "gate": gate,
        "errors": errors,
        "warnings": warnings,
        "refs": {
            "base": {
                "ref": metadata.get("base_ref"),
                "sha": metadata.get("base_sha"),
            },
            "head": {
                "ref": metadata.get("head_ref"),
                "sha": metadata.get("head_sha"),
            },
        },
        "baseline": baseline,
        "tool_versions": metadata.get("tool_versions", {}),
        "commands": metadata.get("commands", {}),
        "benchmarks": {
            "base": load_json(label_paths(root, "base")["benchmarks"], {}),
            "head": load_json(label_paths(root, "head")["benchmarks"], {}),
        },
        "memory_stability": {
            "base": load_json(label_paths(root, "base")["memory_summary"], {}),
            "head": load_json(label_paths(root, "head")["memory_summary"], {}),
        },
        "direct_trace_summaries": {
            "base": load_trace_summaries(root, "base"),
            "head": load_trace_summaries(root, "head"),
        },
        "benchmark_math": {
            "base": load_json(label_paths(root, "base")["math"], {}),
            "head": head_math,
            "head_slices": load_json(label_paths(root, "head")["slices"], {}),
        },
        "classification": classifications,
        "profile_summary": profile if isinstance(profile, dict) else {},
        "artifacts": {
            "root": str(root),
            "classification": str(root / "classification.json"),
            "profile_summary": str(root / "profile_summary.json"),
        },
    }
    return packet


def render_markdown(packet: dict[str, Any]) -> str:
    lines = [
        f"# Perf Frontier Packet: {packet['status'].upper()}",
        "",
        f"- Base: `{nested(packet, 'refs', 'base', 'sha', default='?')}`",
        f"- Head: `{nested(packet, 'refs', 'head', 'sha', default='?')}`",
        f"- Generated: `{packet['generated_at']}`",
    ]
    baseline = packet.get("baseline", {})
    if isinstance(baseline, dict) and baseline:
        lines.append(
            f"- Baseline reference: `{baseline.get('input_path')}` "
            f"sha=`{baseline.get('baseline_sha', 'missing')}`"
        )
    lines.extend(["", "## Gate Summary"])
    if packet["errors"]:
        lines.extend(f"- FAIL: {error}" for error in packet["errors"])
    else:
        lines.append("- PASS: all hard gates passed")
    lines.extend(f"- WARN: {warning}" for warning in packet["warnings"])

    lines.extend(
        [
            "",
            "## Required Classifications",
            "",
            "| Row | Class | Evidence |",
            "|---|---|---|",
        ]
    )
    for row in REQUIRED_BENCHMARK_ROWS:
        entry = packet.get("classification", {}).get(row, {})
        evidence = entry.get("evidence", {}) if isinstance(entry, dict) else {}
        lines.append(
            f"| `{row}` | {entry.get('class', 'missing') if isinstance(entry, dict) else 'missing'} "
            f"| `{json.dumps(evidence, sort_keys=True)[:180]}` |"
        )

    math = nested(packet, "benchmark_math", "head", default={})
    lines.extend(
        [
            "",
            "## Typed Math",
            "",
            f"- Full benchmark checksum gate: `{math.get('checksum_gate', 'missing') if isinstance(math, dict) else 'missing'}`",
            f"- Full benchmark Perry/Node ratio: `{math.get('perry_to_node_ratio') if isinstance(math, dict) else None}`",
        ]
    )
    profile = packet.get("profile_summary", {})
    if isinstance(profile, dict):
        lines.append(
            f"- Profiled row: `{profile.get('row', 'missing')}` via `{profile.get('tool', 'missing')}`"
        )
        for row in profile.get("top_non_gc_costs", [])[:3]:
            if isinstance(row, dict):
                lines.append(
                    f"  - `{row.get('symbol')}` samples={row.get('samples')}"
                )
    lines.append("")
    return "\n".join(lines)


def baseline_from_packet(packet: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "generated_at": packet["generated_at"],
        "baseline_sha": nested(packet, "refs", "head", "sha"),
        "comparison_base_sha": nested(packet, "refs", "base", "sha"),
        "selected_rows": list(REQUIRED_BENCHMARK_ROWS),
        "classification": packet.get("classification", {}),
        "correctness_status": {
            name: nested(entry, "correctness", "status", default="missing")
            for name, entry in benchmark_rows(
                nested(packet, "benchmarks", "head", default={})
            ).items()
        },
        "tool_versions": packet.get("tool_versions", {}),
        "artifact_paths": packet.get("artifacts", {}),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd")

    trace_parser = sub.add_parser("summarize-trace")
    trace_parser.add_argument("--trace", required=True)
    trace_parser.add_argument("--stdout")
    trace_parser.add_argument("--workload", required=True)
    trace_parser.add_argument("--json-out", required=True)
    trace_parser.add_argument("--copied-trace-path")

    math_parser = sub.add_parser("math-json")
    math_parser.add_argument("--repo-root", required=True)
    math_parser.add_argument("--label", required=True)
    math_parser.add_argument("--out-dir", required=True)
    math_parser.add_argument("--math-json-out", required=True)
    math_parser.add_argument("--slice-json-out", required=True)
    math_parser.add_argument("--results-md-out")

    profile_parser = sub.add_parser("profile-summary")
    profile_parser.add_argument("--raw", required=True)
    profile_parser.add_argument("--row", required=True)
    profile_parser.add_argument("--tool", required=True)
    profile_parser.add_argument("--source", required=True)
    profile_parser.add_argument("--json-out", required=True)

    packet_parser = sub.add_parser("packet")
    packet_parser.add_argument("--root", required=True)
    packet_parser.add_argument("--json-out")
    packet_parser.add_argument("--md-out")
    packet_parser.add_argument("--classification-out")
    packet_parser.add_argument("--baseline-in")
    packet_parser.add_argument("--baseline-out")
    packet_parser.add_argument("--gate", action="store_true")

    args = parser.parse_args(argv)
    if args.cmd == "summarize-trace":
        summary = summarize_gc_trace(
            Path(args.trace),
            workload=args.workload,
            stdout_path=Path(args.stdout) if args.stdout else None,
            out_trace_path=Path(args.copied_trace_path) if args.copied_trace_path else None,
        )
        write_json(Path(args.json_out), summary)
        return 0

    if args.cmd == "math-json":
        repo_root = Path(args.repo_root)
        out_dir = Path(args.out_dir)
        math = math_benchmark_json(repo_root, args.label, out_dir)
        slices = slice_results_json(repo_root, args.label, out_dir)
        write_json(Path(args.math_json_out), math)
        write_json(Path(args.slice_json_out), slices)
        if args.results_md_out:
            Path(args.results_md_out).write_text(
                render_math_results(math, slices, utc_now()),
                encoding="utf-8",
            )
        return 0 if math.get("checksum_gate") == "pass" else 1

    if args.cmd == "profile-summary":
        summary = profile_summary(
            raw_path=Path(args.raw),
            row_name=args.row,
            tool=args.tool,
            source=args.source,
        )
        write_json(Path(args.json_out), summary)
        return 0 if summary["status"] == "pass" else 1

    if args.cmd == "packet":
        root = Path(args.root)
        packet = collect_report(
            root,
            gate=args.gate,
            baseline_in=Path(args.baseline_in) if args.baseline_in else None,
        )
        json_out = Path(args.json_out) if args.json_out else root / "perf-frontier-packet.json"
        md_out = Path(args.md_out) if args.md_out else root / "perf-frontier-packet.md"
        classification_out = (
            Path(args.classification_out)
            if args.classification_out
            else root / "classification.json"
        )
        write_json(json_out, packet)
        write_json(classification_out, packet["classification"])
        md_out.parent.mkdir(parents=True, exist_ok=True)
        md_out.write_text(render_markdown(packet), encoding="utf-8")
        if args.baseline_out:
            write_json(Path(args.baseline_out), baseline_from_packet(packet))
        return 1 if packet["status"] == "fail" else 0

    parser.print_help()
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
