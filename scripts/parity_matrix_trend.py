#!/usr/bin/env python3
"""Build and check the per-module parity matrix trend artifact (#812).

The parity runner compares Perry output with Node output per test and writes
`test-parity/reports/latest.json`. This script narrows that report to the
top-level `test_parity_<module>` matrix, computes per-module line/diff counts
from the captured output files, and checks the result against a committed
baseline.
"""

from __future__ import annotations

import argparse
import difflib
import json
import sys
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPORT = REPO_ROOT / "test-parity" / "reports" / "latest.json"
DEFAULT_KNOWN = REPO_ROOT / "test-parity" / "known_failures.json"
DEFAULT_BASELINE = REPO_ROOT / "test-parity" / "parity_matrix_baseline.json"
DEFAULT_OUTPUT_DIR = REPO_ROOT / "test-parity" / "output"
DEFAULT_JSON = REPO_ROOT / "test-parity" / "reports" / "parity_matrix_latest.json"
DEFAULT_MARKDOWN = REPO_ROOT / "test-parity" / "reports" / "parity_matrix_latest.md"

FAIL_STATUSES = {"parity_fail", "compile_fail", "node_fail"}


@dataclass
class ModuleRecord:
    module: str
    test_id: str
    status: str
    known: bool
    node_lines: int | None
    perry_lines: int | None
    diff_lines: int | None


def load_json(path: Path) -> dict:
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


def normalize_status(status: str) -> str:
    if status == "fail":
        return "parity_fail"
    return status


def module_from_test_id(test_id: str) -> str | None:
    prefix = "test_parity_"
    if not test_id.startswith(prefix):
        return None
    return test_id[len(prefix):]


def safe_test_id(test_id: str) -> str:
    return test_id.replace("/", "__")


def read_lines(path: Path) -> list[str] | None:
    if not path.exists():
        return None
    return path.read_text(encoding="utf-8", errors="replace").splitlines()


def diff_line_count(node_lines: list[str] | None, perry_lines: list[str] | None) -> int | None:
    if node_lines is None or perry_lines is None:
        return None
    diff = difflib.unified_diff(node_lines, perry_lines, n=0, lineterm="")
    count = 0
    for line in diff:
        if line.startswith(("+++", "---", "@@")):
            continue
        if line.startswith(("+", "-")):
            count += 1
    return count


def known_failures(path: Path) -> set[str]:
    if not path.exists():
        return set()
    data = load_json(path)
    return {key for key in data if key != "_schema"}


def report_results(report: dict) -> list[dict[str, str]]:
    results = report.get("results")
    if isinstance(results, list):
        out: list[dict[str, str]] = []
        for item in results:
            if not isinstance(item, dict):
                continue
            test_id = item.get("id")
            status = item.get("status")
            if isinstance(test_id, str) and isinstance(status, str):
                out.append({"id": test_id, "status": normalize_status(status)})
        return out

    # Backward compatibility for reports generated before run_parity_tests.sh
    # started emitting the `results` array. This can only describe failures.
    out = []
    failures = report.get("failures", {})
    for test_id in failures.get("parity", []) or []:
        if test_id:
            out.append({"id": test_id, "status": "parity_fail"})
    for test_id in failures.get("compile", []) or []:
        if test_id:
            out.append({"id": test_id, "status": "compile_fail"})
    return out


def build_records(report: dict, known: set[str], output_dir: Path) -> list[ModuleRecord]:
    records: list[ModuleRecord] = []
    seen: set[str] = set()
    for result in report_results(report):
        test_id = result["id"]
        module = module_from_test_id(test_id)
        if module is None or test_id in seen:
            continue
        seen.add(test_id)
        safe = safe_test_id(test_id)
        node_lines = read_lines(output_dir / "node" / f"{safe}.txt")
        perry_lines = read_lines(output_dir / "perry" / f"{safe}.txt")
        records.append(ModuleRecord(
            module=module,
            test_id=test_id,
            status=result["status"],
            known=test_id in known,
            node_lines=None if node_lines is None else len(node_lines),
            perry_lines=None if perry_lines is None else len(perry_lines),
            diff_lines=diff_line_count(node_lines, perry_lines),
        ))
    return sorted(records, key=lambda record: record.module)


def load_baseline(path: Path) -> dict:
    if not path.exists():
        return {"modules": {}}
    data = load_json(path)
    if not isinstance(data.get("modules"), dict):
        raise ValueError(f"{path} must contain a top-level modules object")
    return data


def allowed_statuses(config: dict | None) -> set[str]:
    if config is None:
        return {"pass"}
    raw = config.get("allowed_statuses", ["pass"])
    if not isinstance(raw, list) or not all(isinstance(item, str) for item in raw):
        raise ValueError("allowed_statuses must be a string array")
    return set(raw) | {"pass"}


def check_records(records: list[ModuleRecord], baseline: dict) -> list[str]:
    problems: list[str] = []
    modules = baseline.get("modules", {})

    for record in records:
        config = modules.get(record.module)
        if record.status in FAIL_STATUSES and not record.known:
            problems.append(
                f"{record.test_id}: {record.status} is not listed in known_failures.json"
            )

        allowed = allowed_statuses(config)
        if record.status not in allowed:
            problems.append(
                f"{record.test_id}: status {record.status} is outside baseline "
                f"allowed statuses {sorted(allowed)}"
            )

        max_diff = 0 if config is None else config.get("max_diff_lines", 0)
        if max_diff is not None and not isinstance(max_diff, int):
            raise ValueError(f"{record.module}.max_diff_lines must be an integer or null")
        if (
            isinstance(max_diff, int)
            and record.diff_lines is not None
            and record.diff_lines > max_diff
        ):
            problems.append(
                f"{record.test_id}: diff_lines {record.diff_lines} exceeds baseline {max_diff}"
            )

    return problems


def write_json(path: Path, records: list[ModuleRecord], problems: list[str], source: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    failing = [record for record in records if record.status in FAIL_STATUSES]
    payload = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "source_report": str(source),
        "summary": {
            "modules": len(records),
            "passing": sum(1 for record in records if record.status == "pass"),
            "failing": len(failing),
            "known_failures": sum(1 for record in failing if record.known),
            "problems": len(problems),
        },
        "modules": [asdict(record) for record in records],
        "problems": problems,
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def markdown_table(records: list[ModuleRecord], problems: list[str]) -> str:
    lines = [
        "# Parity Matrix Trend",
        "",
        "| module | status | known | node_lines | perry_lines | diff_lines |",
        "| --- | --- | --- | ---: | ---: | ---: |",
    ]
    for record in records:
        def fmt(value: int | None) -> str:
            return "" if value is None else str(value)

        lines.append(
            f"| {record.module} | {record.status} | "
            f"{'yes' if record.known else 'no'} | "
            f"{fmt(record.node_lines)} | {fmt(record.perry_lines)} | "
            f"{fmt(record.diff_lines)} |"
        )
    if problems:
        lines.extend(["", "## Problems", ""])
        lines.extend(f"- {problem}" for problem in problems)
    return "\n".join(lines) + "\n"


def write_markdown(path: Path, records: list[ModuleRecord], problems: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown_table(records, problems), encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--known", type=Path, default=DEFAULT_KNOWN)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--output-json", type=Path, default=DEFAULT_JSON)
    parser.add_argument("--output-md", type=Path, default=DEFAULT_MARKDOWN)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)

    report = load_json(args.report)
    known = known_failures(args.known)
    baseline = load_baseline(args.baseline)
    records = build_records(report, known, args.output_dir)
    problems = check_records(records, baseline) if args.check else []

    write_json(args.output_json, records, problems, args.report)
    write_markdown(args.output_md, records, problems)
    sys.stdout.write(markdown_table(records, problems))

    if args.check and problems:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
