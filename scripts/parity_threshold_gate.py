#!/usr/bin/env python3
"""Check global and per-category parity thresholds.

`run_parity_tests.sh` writes `test-parity/reports/latest.json` with one
compact result per test. This gate keeps the historical global parity minimum
and adds category-level minima so one passing domain cannot hide another
domain's regression in the aggregate percentage.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPORT = REPO_ROOT / "test-parity" / "reports" / "latest.json"
DEFAULT_THRESHOLDS = REPO_ROOT / "test-parity" / "threshold.json"
DEFAULT_JSON = REPO_ROOT / "test-parity" / "reports" / "parity_threshold_latest.json"
DEFAULT_MARKDOWN = REPO_ROOT / "test-parity" / "reports" / "parity_threshold_latest.md"

FAIL_STATUSES = {"parity_fail", "compile_fail"}


@dataclass
class CategoryRecord:
    category: str
    threshold: float
    pass_count: int
    parity_fail: int
    compile_fail: int
    node_fail: int
    skipped: int
    total_counted: int
    parity_percentage: float | None
    configured: bool


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as fh:
        data = json.load(fh)
    if not isinstance(data, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return data


def as_float(value: Any, field: str) -> float:
    if not isinstance(value, (int, float)):
        raise ValueError(f"{field} must be numeric")
    return float(value)


def category_for_test_id(test_id: str) -> str:
    if test_id.startswith("node-suite/"):
        parts = test_id.split("/")
        if len(parts) >= 2 and parts[1]:
            return f"node-suite/{parts[1]}"
        return "node-suite"

    prefix = "test_parity_"
    if test_id.startswith(prefix):
        return f"parity/{test_id[len(prefix):]}"

    if test_id.startswith("test_gap_"):
        return "legacy/gap"
    if test_id.startswith("test_issue_"):
        return "legacy/issue"
    if test_id.startswith("test_feat_"):
        return "legacy/feature"
    return "legacy/other"


def report_results(report: dict[str, Any]) -> list[dict[str, str]]:
    results = report.get("results")
    if not isinstance(results, list):
        return []

    out: list[dict[str, str]] = []
    for item in results:
        if not isinstance(item, dict):
            continue
        test_id = item.get("id")
        status = item.get("status")
        if isinstance(test_id, str) and isinstance(status, str):
            if status == "fail":
                status = "parity_fail"
            out.append({"id": test_id, "status": status})
    return out


def threshold_config(thresholds: dict[str, Any]) -> tuple[float, dict[str, float]]:
    default_category = as_float(
        thresholds.get("default_category_min_parity_pct", thresholds.get("min_parity_pct", 0.0)),
        "default_category_min_parity_pct",
    )
    raw_categories = thresholds.get("categories", {})
    if not isinstance(raw_categories, dict):
        raise ValueError("categories must be an object when present")

    categories: dict[str, float] = {}
    for category, config in raw_categories.items():
        if not isinstance(category, str):
            raise ValueError("category names must be strings")
        if isinstance(config, dict):
            value = config.get("min_parity_pct")
        else:
            value = config
        categories[category] = as_float(value, f"categories.{category}.min_parity_pct")
    return default_category, categories


def build_category_records(
    results: list[dict[str, str]],
    default_threshold: float,
    configured_thresholds: dict[str, float],
) -> list[CategoryRecord]:
    buckets: dict[str, dict[str, int]] = {}
    for result in results:
        category = category_for_test_id(result["id"])
        status = result["status"]
        counts = buckets.setdefault(
            category,
            {"pass": 0, "parity_fail": 0, "compile_fail": 0, "node_fail": 0, "skipped": 0},
        )
        if status in counts:
            counts[status] += 1

    records: list[CategoryRecord] = []
    for category, counts in sorted(buckets.items()):
        total = counts["pass"] + counts["parity_fail"] + counts["compile_fail"]
        pct = None if total == 0 else round(counts["pass"] * 100.0 / total, 1)
        configured = category in configured_thresholds
        records.append(CategoryRecord(
            category=category,
            threshold=configured_thresholds.get(category, default_threshold),
            pass_count=counts["pass"],
            parity_fail=counts["parity_fail"],
            compile_fail=counts["compile_fail"],
            node_fail=counts["node_fail"],
            skipped=counts["skipped"],
            total_counted=total,
            parity_percentage=pct,
            configured=configured,
        ))
    return records


def check_global(report: dict[str, Any], thresholds: dict[str, Any]) -> list[str]:
    summary = report.get("summary", {})
    if not isinstance(summary, dict):
        return ["report summary is missing or not an object"]
    actual = as_float(summary.get("parity_percentage"), "report.summary.parity_percentage")
    minimum = as_float(thresholds.get("min_parity_pct"), "min_parity_pct")
    if actual < minimum:
        return [f"global parity {actual:.1f}% is below threshold {minimum:.1f}%"]
    return []


def check_categories(records: list[CategoryRecord]) -> list[str]:
    problems: list[str] = []
    for record in records:
        if record.parity_percentage is None:
            continue
        if record.parity_percentage < record.threshold:
            problems.append(
                f"{record.category}: parity {record.parity_percentage:.1f}% "
                f"is below threshold {record.threshold:.1f}%"
            )
    return problems


def write_json(
    path: Path,
    report_path: Path,
    threshold_path: Path,
    records: list[CategoryRecord],
    problems: list[str],
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "source_report": str(report_path),
        "source_thresholds": str(threshold_path),
        "summary": {
            "categories": len(records),
            "problems": len(problems),
        },
        "categories": [asdict(record) for record in records],
        "problems": problems,
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def markdown(records: list[CategoryRecord], problems: list[str]) -> str:
    lines = [
        "# Parity Threshold Gate",
        "",
        "| category | parity | threshold | pass | parity_fail | compile_fail | node_fail | skipped | source |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
    ]
    for record in records:
        pct = "" if record.parity_percentage is None else f"{record.parity_percentage:.1f}%"
        source = "configured" if record.configured else "default"
        lines.append(
            f"| {record.category} | {pct} | {record.threshold:.1f}% | "
            f"{record.pass_count} | {record.parity_fail} | {record.compile_fail} | "
            f"{record.node_fail} | {record.skipped} | {source} |"
        )
    if problems:
        lines.extend(["", "## Problems", ""])
        lines.extend(f"- {problem}" for problem in problems)
    return "\n".join(lines) + "\n"


def write_markdown(path: Path, records: list[CategoryRecord], problems: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown(records, problems), encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--thresholds", type=Path, default=DEFAULT_THRESHOLDS)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_MARKDOWN)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)

    report = load_json(args.report)
    thresholds = load_json(args.thresholds)
    default_threshold, category_thresholds = threshold_config(thresholds)
    records = build_category_records(report_results(report), default_threshold, category_thresholds)

    problems = check_global(report, thresholds) + check_categories(records)
    write_json(args.output_json, args.report, args.thresholds, records, problems)
    write_markdown(args.output_md, records, problems)
    sys.stdout.write(markdown(records, problems))

    if args.check and problems:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
