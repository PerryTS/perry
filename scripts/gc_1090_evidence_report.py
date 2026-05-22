#!/usr/bin/env python3
"""Build a PR-ready #1090 GC evidence packet from exact-head artifacts."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REQUIRED_BENCHMARKS = (
    "bench_json_roundtrip",
    "bench_gc_pressure",
    "07_object_create",
    "12_binary_trees",
)

STRICT_COPIED_MINOR_WORKLOADS = (
    "json_roundtrip",
    "string_churn",
    "object_property_churn",
    "mixed_request_shaping",
    "map_set_churn",
    "promise_churn",
)

FALLBACK_REASONS = (
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

SPEED_THRESHOLD_PCT = 15.0
MEMORY_THRESHOLD_PCT = 25.0
MIN_SPEED_DELTA_MS = 20
MIN_MEMORY_DELTA_KB = 2048


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


def int_value(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    return 0


def pct_delta(base: int | None, head: int | None) -> float | None:
    if base is None or head is None or base <= 0:
        return None
    return ((head - base) / base) * 100.0


def ratio_delta(base: int | None, head: int | None) -> dict[str, Any]:
    pct = pct_delta(base, head)
    return {
        "base": base,
        "head": head,
        "delta": None if base is None or head is None else head - base,
        "delta_pct": None if pct is None else round(pct, 1),
    }


def command_exit(metadata: dict[str, Any], label: str, command: str) -> int | None:
    exit_code = nested(metadata, "commands", label, command, "exit_code")
    return exit_code if isinstance(exit_code, int) else None


def command_status(metadata: dict[str, Any], label: str, command: str) -> str:
    status = nested(metadata, "commands", label, command, "status")
    if isinstance(status, str):
        return status
    exit_code = command_exit(metadata, label, command)
    if exit_code is None:
        return "missing"
    return "pass" if exit_code == 0 else "fail"


def label_paths(root: Path, label: str) -> dict[str, Path]:
    base = root / label
    return {
        "benchmarks": base / "benchmarks" / "full.json",
        "memory_summary": base / "memory" / "reports" / "memory_stability_summary.json",
        "copied_minor": base / "memory" / "reports" / "copied_minor_fallback_report.json",
        "target_collector": base / "memory" / "reports" / "target_collector_gates_report.json",
    }


def memory_summary(root: Path, label: str) -> dict[str, Any]:
    summary = load_json(label_paths(root, label)["memory_summary"], {})
    return {
        "passed": int_value(summary.get("passed")) if isinstance(summary, dict) else 0,
        "failed": int_value(summary.get("failed")) if isinstance(summary, dict) else 0,
        "skipped": int_value(summary.get("skipped")) if isinstance(summary, dict) else 0,
        "path": str(label_paths(root, label)["memory_summary"]),
        "present": bool(summary),
    }


def benchmark_entry(benchmarks: dict[str, Any], name: str) -> dict[str, Any]:
    entry = nested(benchmarks, "benchmarks", name, default={})
    return entry if isinstance(entry, dict) else {}


def benchmark_matrix(
    root: Path,
    base_label: str,
    head_label: str,
    errors: list[str],
    warnings: list[str],
) -> dict[str, Any]:
    base = load_json(label_paths(root, base_label)["benchmarks"], {})
    head = load_json(label_paths(root, head_label)["benchmarks"], {})
    matrix: dict[str, Any] = {}

    for report_label, report in ((base_label, base), (head_label, head)):
        if not report:
            errors.append(f"{report_label}: benchmark JSON is missing")
            continue
        for name, entry in nested(report, "benchmarks", default={}).items():
            correctness = entry.get("correctness", {})
            status = correctness.get("status")
            if status == "fail":
                errors.append(
                    f"{report_label}:{name}: correctness failed: "
                    f"{correctness.get('reason', 'semantic output mismatch')}"
                )
            elif status == "unchecked":
                warnings.append(f"{report_label}:{name}: correctness unchecked")

    for name in REQUIRED_BENCHMARKS:
        base_entry = benchmark_entry(base, name)
        head_entry = benchmark_entry(head, name)
        if not base_entry:
            errors.append(f"{base_label}:{name}: required benchmark missing")
        if not head_entry:
            errors.append(f"{head_label}:{name}: required benchmark missing")

        base_ms = base_entry.get("perry_ms")
        head_ms = head_entry.get("perry_ms")
        base_rss = base_entry.get("perry_rss_kb")
        head_rss = head_entry.get("perry_rss_kb")
        time = ratio_delta(base_ms, head_ms)
        rss = ratio_delta(base_rss, head_rss)

        time_regression = (
            time["delta_pct"] is not None
            and time["delta_pct"] > SPEED_THRESHOLD_PCT
            and abs(time["delta"] or 0) >= MIN_SPEED_DELTA_MS
        )
        rss_regression = (
            rss["delta_pct"] is not None
            and rss["delta_pct"] > MEMORY_THRESHOLD_PCT
            and abs(rss["delta"] or 0) >= MIN_MEMORY_DELTA_KB
        )
        if time_regression:
            errors.append(
                f"{name}: time regression {base_ms}ms -> {head_ms}ms "
                f"({time['delta_pct']:+.1f}%)"
            )
        if rss_regression:
            errors.append(
                f"{name}: RSS regression {base_rss}KB -> {head_rss}KB "
                f"({rss['delta_pct']:+.1f}%)"
            )

        matrix[name] = {
            "time_ms": time,
            "rss_kb": rss,
            "base_correctness": nested(base_entry, "correctness", "status", default="missing"),
            "head_correctness": nested(head_entry, "correctness", "status", default="missing"),
            "gate": "fail" if time_regression or rss_regression else "pass",
        }

    return matrix


def normalize_reason_counts(counts: Any) -> dict[str, int]:
    result = {reason: 0 for reason in FALLBACK_REASONS}
    if isinstance(counts, dict):
        for key, value in counts.items():
            if isinstance(key, str):
                result[key] = int_value(value)
    return result


def copied_report_summary(root: Path, label: str) -> dict[str, Any]:
    report = load_json(label_paths(root, label)["copied_minor"], {})
    summary = report.get("summary", {}) if isinstance(report, dict) else {}
    workloads = report.get("workloads", {}) if isinstance(report, dict) else {}
    return {
        "present": bool(report),
        "path": str(label_paths(root, label)["copied_minor"]),
        "summary": {
            "cycles": int_value(summary.get("cycles")) if isinstance(summary, dict) else 0,
            "fallback_reason_counts": normalize_reason_counts(
                summary.get("fallback_reason_counts") if isinstance(summary, dict) else {}
            ),
            "conservative_pinned_bytes": int_value(
                summary.get("conservative_pinned_bytes") if isinstance(summary, dict) else 0
            ),
            "legacy_copy_only_scanner_pinned_bytes": int_value(
                nested(summary, "legacy_copy_only_scanner_pinned", "bytes", default=0)
            ),
            "copied_objects": int_value(nested(summary, "copying_nursery", "copied_objects", default=0)),
            "copied_bytes": int_value(nested(summary, "copying_nursery", "copied_bytes", default=0)),
            "promoted_objects": int_value(nested(summary, "copying_nursery", "promoted_objects", default=0)),
            "promoted_bytes": int_value(nested(summary, "copying_nursery", "promoted_bytes", default=0)),
            "malloc_registry_rebuilds": int_value(
                nested(summary, "copying_nursery", "malloc_registry_rebuilds", default=0)
            ),
        },
        "workloads": workloads if isinstance(workloads, dict) else {},
    }


def target_collector_summary(root: Path, label: str) -> dict[str, Any]:
    report = load_json(label_paths(root, label)["target_collector"], {})
    summary = report.get("summary", {}) if isinstance(report, dict) else {}
    return {
        "present": bool(report),
        "path": str(label_paths(root, label)["target_collector"]),
        "cycles": int_value(summary.get("cycles")) if isinstance(summary, dict) else 0,
        "fallback_reason_counts": normalize_reason_counts(
            summary.get("fallback_reason_counts") if isinstance(summary, dict) else {}
        ),
        "copied_objects": int_value(nested(summary, "copying_nursery", "copied_objects", default=0)),
        "copied_bytes": int_value(nested(summary, "copying_nursery", "copied_bytes", default=0)),
        "promoted_objects": int_value(nested(summary, "copying_nursery", "promoted_objects", default=0)),
        "promoted_bytes": int_value(nested(summary, "copying_nursery", "promoted_bytes", default=0)),
        "malloc_registry_rebuilds": int_value(
            nested(summary, "copying_nursery", "malloc_registry_rebuilds", default=0)
        ),
        "old_page_accounting": summary.get("old_page_accounting", {})
        if isinstance(summary, dict)
        else {},
    }


def workload_counts(workload: dict[str, Any]) -> dict[str, Any]:
    return {
        "fallback_reason_counts": normalize_reason_counts(
            workload.get("fallback_reason_counts", {})
        ),
        "conservative_pinned_bytes": int_value(workload.get("conservative_pinned_bytes")),
        "legacy_copy_only_scanner_pinned_bytes": int_value(
            nested(workload, "legacy_copy_only_scanner_pinned", "bytes", default=0)
        ),
        "malloc_registry_rebuilds": int_value(
            nested(workload, "copying_nursery", "malloc_registry_rebuilds", default=0)
        ),
        "copied_objects": int_value(
            nested(workload, "copying_nursery", "copied_objects", default=0)
        ),
        "promoted_objects": int_value(
            nested(workload, "copying_nursery", "promoted_objects", default=0)
        ),
    }


def gate_copied_minor(
    head_copied: dict[str, Any],
    errors: list[str],
    warnings: list[str],
) -> dict[str, Any]:
    if not head_copied["present"]:
        errors.append("head: copied-minor fallback report is missing")
        return {}

    workload_results: dict[str, Any] = {}
    workloads = head_copied["workloads"]
    for name in STRICT_COPIED_MINOR_WORKLOADS:
        workload = workloads.get(name)
        if not isinstance(workload, dict):
            warnings.append(f"head:{name}: strict copied-minor workload missing")
            continue
        counts = workload_counts(workload)
        workload_results[name] = counts
        non_none = {
            reason: count
            for reason, count in counts["fallback_reason_counts"].items()
            if reason != "none" and count > 0
        }
        if name == "json_roundtrip" and non_none:
            errors.append(f"head:{name}: fallback reasons other than none: {non_none}")
        if counts["conservative_pinned_bytes"] != 0:
            errors.append(
                f"head:{name}: conservative_pinned_bytes="
                f"{counts['conservative_pinned_bytes']}, want 0"
            )
        if counts["legacy_copy_only_scanner_pinned_bytes"] != 0:
            errors.append(
                f"head:{name}: legacy_copy_only_scanner_pinned.bytes="
                f"{counts['legacy_copy_only_scanner_pinned_bytes']}, want 0"
            )
        if counts["malloc_registry_rebuilds"] != 0:
            errors.append(
                f"head:{name}: malloc_registry_rebuilds="
                f"{counts['malloc_registry_rebuilds']}, want 0"
            )

    return workload_results


def perf_summary(metadata: dict[str, Any], base_label: str, head_label: str) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for label in (base_label, head_label):
        entry = nested(metadata, "commands", label, "perf_comprehensive", default={})
        if not isinstance(entry, dict):
            entry = {}
        entry = dict(entry)
        log = entry.get("log")
        outlier_lines: list[str] = []
        if isinstance(log, str) and log:
            log_path = Path(log)
            if log_path.exists():
                for line in log_path.read_text(
                    encoding="utf-8", errors="replace"
                ).splitlines():
                    lowered = line.lower()
                    if "gc" in lowered or "outlier" in lowered:
                        outlier_lines.append(line)
                    if len(outlier_lines) >= 20:
                        break
        entry["outlier_lines"] = outlier_lines
        result[label] = entry
    return result


def collect_report(root: Path, base_label: str, head_label: str) -> dict[str, Any]:
    metadata = load_json(root / "metadata.json", {})
    errors: list[str] = []
    warnings: list[str] = []

    for label in (base_label, head_label):
        if command_exit(metadata, label, "build") not in (0, None):
            errors.append(f"{label}: release build failed")
        memory_exit = command_exit(metadata, label, "memory_stability")
        if memory_exit not in (0, None):
            errors.append(f"{label}: memory stability command failed with {memory_exit}")
        bench_exit = command_exit(metadata, label, "benchmarks")
        if bench_exit not in (0, None):
            warnings.append(
                f"{label}: benchmark command exited {bench_exit}; "
                "required benchmark gates are evaluated from JSON"
            )

    memory = {
        base_label: memory_summary(root, base_label),
        head_label: memory_summary(root, head_label),
    }
    for label, summary in memory.items():
        if not summary["present"]:
            errors.append(f"{label}: memory stability summary missing")
        if summary["failed"] != 0:
            errors.append(f"{label}: memory stability failed={summary['failed']}")

    benchmarks = benchmark_matrix(root, base_label, head_label, errors, warnings)
    copied_minor = {
        base_label: copied_report_summary(root, base_label),
        head_label: copied_report_summary(root, head_label),
    }
    target_collector = {
        base_label: target_collector_summary(root, base_label),
        head_label: target_collector_summary(root, head_label),
    }
    strict_workloads = gate_copied_minor(copied_minor[head_label], errors, warnings)

    perf = perf_summary(metadata, base_label, head_label)

    packet = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "status": "fail" if errors else "pass",
        "errors": errors,
        "warnings": warnings,
        "refs": {
            "base": {
                "label": base_label,
                "ref": metadata.get("base_ref"),
                "sha": metadata.get("base_sha"),
            },
            "head": {
                "label": head_label,
                "ref": metadata.get("head_ref"),
                "sha": metadata.get("head_sha"),
            },
        },
        "commands": metadata.get("commands", {}),
        "memory_stability": memory,
        "benchmarks": benchmarks,
        "copied_minor": copied_minor,
        "strict_head_workloads": strict_workloads,
        "target_collector": target_collector,
        "perf_comprehensive": perf,
    }
    return packet


def fmt_delta(entry: dict[str, Any], unit: str) -> str:
    base = entry.get("base")
    head = entry.get("head")
    delta = entry.get("delta")
    pct = entry.get("delta_pct")
    if base is None or head is None or delta is None or pct is None:
        return "missing"
    sign = "+" if delta >= 0 else ""
    return f"{base}{unit} -> {head}{unit} ({sign}{delta}{unit}, {pct:+.1f}%)"


def reason_summary(counts: dict[str, int]) -> str:
    nonzero = {key: value for key, value in counts.items() if value}
    if not nonzero:
        return "none"
    return ", ".join(f"{key}={value}" for key, value in sorted(nonzero.items()))


def render_markdown(packet: dict[str, Any]) -> str:
    status = packet["status"].upper()
    base_sha = nested(packet, "refs", "base", "sha", default="?")
    head_sha = nested(packet, "refs", "head", "sha", default="?")
    lines = [
        f"# #1090 GC Evidence Packet: {status}",
        "",
        f"- Base: `{base_sha}`",
        f"- Head: `{head_sha}`",
        f"- Generated: `{packet['generated_at']}`",
        "",
        "## Gate Summary",
    ]
    if packet["errors"]:
        lines.extend(f"- FAIL: {error}" for error in packet["errors"])
    else:
        lines.append("- PASS: all hard gates passed")
    if packet["warnings"]:
        lines.extend(f"- WARN: {warning}" for warning in packet["warnings"])

    lines.extend(
        [
            "",
            "## Required Benchmarks",
            "",
            "| Benchmark | Correct | Time | RSS | Gate |",
            "|---|---|---:|---:|---|",
        ]
    )
    for name in REQUIRED_BENCHMARKS:
        entry = packet["benchmarks"].get(name, {})
        correct = f"{entry.get('base_correctness', '?')} -> {entry.get('head_correctness', '?')}"
        lines.append(
            f"| `{name}` | {correct} | {fmt_delta(entry.get('time_ms', {}), 'ms')} "
            f"| {fmt_delta(entry.get('rss_kb', {}), 'KB')} | {entry.get('gate', 'missing')} |"
        )

    lines.extend(["", "## Memory Stability", "", "| Ref | Passed | Failed | Skipped |"])
    lines.append("|---|---:|---:|---:|")
    for label, summary in packet["memory_stability"].items():
        lines.append(
            f"| `{label}` | {summary['passed']} | {summary['failed']} | {summary['skipped']} |"
        )

    lines.extend(
        [
            "",
            "## Copied-Minor Evidence",
            "",
            "| Ref | Fallback Reasons | Conservative Pinned Bytes | Copy-Only Pinned Bytes | Copied/Promoted Objects | Copied/Promoted Bytes | Malloc Registry Rebuilds |",
            "|---|---|---:|---:|---:|---:|---:|",
        ]
    )
    for label, report in packet["copied_minor"].items():
        summary = report["summary"]
        copied_promoted = summary["copied_objects"] + summary["promoted_objects"]
        copied_promoted_bytes = summary["copied_bytes"] + summary["promoted_bytes"]
        lines.append(
            f"| `{label}` | {reason_summary(summary['fallback_reason_counts'])} "
            f"| {summary['conservative_pinned_bytes']} "
            f"| {summary['legacy_copy_only_scanner_pinned_bytes']} "
            f"| {copied_promoted} "
            f"| {copied_promoted_bytes} "
            f"| {summary['malloc_registry_rebuilds']} |"
        )

    lines.extend(
        [
            "",
            "## Target Collector Gates",
            "",
            "| Ref | Present | Fallback Reasons | Copied Objects | Copied Bytes | Promoted Objects | Promoted Bytes | Malloc Registry Rebuilds |",
            "|---|---|---|---:|---:|---:|---:|---:|",
        ]
    )
    for label, report in packet["target_collector"].items():
        lines.append(
            f"| `{label}` | {report['present']} "
            f"| {reason_summary(report['fallback_reason_counts'])} "
            f"| {report['copied_objects']} | {report['copied_bytes']} "
            f"| {report['promoted_objects']} | {report['promoted_bytes']} "
            f"| {report['malloc_registry_rebuilds']} |"
        )

    lines.extend(["", "## Perf-Comprehensive Outlier Check", ""])
    for label, perf in packet["perf_comprehensive"].items():
        status = perf.get("status", "missing") if isinstance(perf, dict) else "missing"
        reason = perf.get("reason", "") if isinstance(perf, dict) else ""
        log = perf.get("log", "") if isinstance(perf, dict) else ""
        suffix = f" ({reason})" if reason else ""
        log_part = f" log: `{log}`" if log else ""
        lines.append(f"- `{label}`: {status}{suffix}{log_part}")
        for outlier in perf.get("outlier_lines", []) if isinstance(perf, dict) else []:
            lines.append(f"  - `{outlier}`")

    lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="Evidence output root")
    parser.add_argument("--base-label", default="base")
    parser.add_argument("--head-label", default="head")
    parser.add_argument("--json-out", help="Packet JSON path")
    parser.add_argument("--md-out", help="Packet Markdown path")
    args = parser.parse_args(argv)

    root = Path(args.root)
    packet = collect_report(root, args.base_label, args.head_label)

    json_out = Path(args.json_out) if args.json_out else root / "gc-1090-packet.json"
    md_out = Path(args.md_out) if args.md_out else root / "gc-1090-packet.md"
    write_json(json_out, packet)
    md_out.parent.mkdir(parents=True, exist_ok=True)
    md_out.write_text(render_markdown(packet), encoding="utf-8")

    return 1 if packet["status"] == "fail" else 0


if __name__ == "__main__":
    raise SystemExit(main())
