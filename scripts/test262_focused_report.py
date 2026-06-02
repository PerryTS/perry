#!/usr/bin/env python3
"""Run the focused Test262 language/expressions parity report.

This is a repeatable wrapper around scripts/test262_subset.py for the first
c262 parity slice: language/expressions, max 500, sample-cap 1000000. It
vendors Test262 and builds Perry when needed, then writes a stable report path
plus a TSV problem list.
"""

from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
TEST262_COMPAT_DIR = REPO_ROOT / "test-compat" / "test262"
PINNED_SHA = TEST262_COMPAT_DIR / "pinned-sha.txt"

DEFAULT_ROOT = REPO_ROOT / "vendor" / "test262"
DEFAULT_REPORTS_DIR = TEST262_COMPAT_DIR / "reports"
DEFAULT_PERRY_BIN = REPO_ROOT / "target" / "release" / "perry"
DEFAULT_DIRS = ("language/expressions",)
DEFAULT_MAX = 500
DEFAULT_SAMPLE_CAP = 1_000_000
PROBLEM_BUCKETS = ("diff", "runtime-fail", "compile-fail")


def run_command(cmd: list[str], *, cwd: Path = REPO_ROOT) -> None:
    print("+ " + " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=cwd, check=True)


def pinned_sha() -> str:
    return PINNED_SHA.read_text(encoding="utf-8").strip()


def ensure_test262(root: Path) -> None:
    if (root / "test").is_dir() and (root / "harness").is_dir():
        return
    if root.exists():
        raise SystemExit(
            f"error: {root} exists but is not a Test262 checkout "
            "(need test/ + harness/)"
        )

    root.parent.mkdir(parents=True, exist_ok=True)
    run_command(
        ["git", "clone", "--depth", "1", "https://github.com/tc39/test262", str(root)]
    )
    run_command(["git", "fetch", "--depth", "1", "origin", pinned_sha()], cwd=root)
    run_command(["git", "checkout", "--detach", "FETCH_HEAD"], cwd=root)


def ensure_perry(perry_bin: Path) -> None:
    if perry_bin.exists():
        return
    run_command(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "perry",
            "-p",
            "perry-runtime",
            "-p",
            "perry-stdlib",
        ]
    )
    if not perry_bin.exists():
        raise SystemExit(f"error: expected Perry binary was not built: {perry_bin}")


def clean_tsv_cell(value: Any) -> str:
    text = "" if value is None else str(value)
    return " ".join(text.replace("\r", "\n").replace("\t", " ").split())


def problem_rows(report: dict[str, Any]) -> list[tuple[str, str, str]]:
    rows: list[tuple[str, str, str]] = []
    samples = report.get("samples", {})
    if not isinstance(samples, dict):
        raise ValueError("report.samples must be an object")

    for bucket in PROBLEM_BUCKETS:
        bucket_samples = samples.get(bucket, [])
        if not isinstance(bucket_samples, list):
            raise ValueError(f"report.samples.{bucket} must be a list")
        for sample in bucket_samples:
            if not isinstance(sample, dict):
                raise ValueError(f"report.samples.{bucket} entries must be objects")
            test = clean_tsv_cell(sample.get("test", ""))
            reason = clean_tsv_cell(sample.get("reason", ""))
            if test:
                rows.append((bucket, test, reason))
    return rows


def assert_untruncated_problem_samples(report: dict[str, Any]) -> None:
    totals = report.get("totals", {})
    samples = report.get("samples", {})
    if not isinstance(totals, dict) or not isinstance(samples, dict):
        raise ValueError("report must contain object totals and samples")

    truncated: list[str] = []
    for bucket in PROBLEM_BUCKETS:
        total = int(totals.get(bucket, 0))
        captured = len(samples.get(bucket, []))
        if captured < total:
            truncated.append(f"{bucket}: captured {captured} of {total}")
    if truncated:
        raise ValueError(
            "problem TSV would be incomplete; rerun with a larger --sample-cap "
            + "; ".join(truncated)
        )


def write_problem_tsv(report_path: Path, tsv_path: Path) -> int:
    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert_untruncated_problem_samples(report)
    rows = problem_rows(report)

    tsv_path.parent.mkdir(parents=True, exist_ok=True)
    with tsv_path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(("bucket", "test", "reason"))
        writer.writerows(rows)
    return len(rows)


def report_stem(dirs: list[str], max_cases: int) -> str:
    if dirs == list(DEFAULT_DIRS):
        return f"focused-language-expressions-{max_cases}"
    safe_dirs = "-".join(d.replace("/", "-") for d in dirs)
    return f"focused-{safe_dirs}-{max_cases}"


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(
        description="Run the focused Test262 c262 parity report command"
    )
    ap.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    ap.add_argument("--perry-bin", type=Path, default=DEFAULT_PERRY_BIN)
    ap.add_argument("--reports-dir", type=Path, default=DEFAULT_REPORTS_DIR)
    ap.add_argument("--dir", nargs="+", default=list(DEFAULT_DIRS))
    ap.add_argument("--max", type=int, default=DEFAULT_MAX)
    ap.add_argument("--sample-cap", type=int, default=DEFAULT_SAMPLE_CAP)
    ap.add_argument("--timeout", type=int, default=20)
    ap.add_argument(
        "--skip-vendor",
        action="store_true",
        help="fail instead of cloning Test262 when --root is missing",
    )
    ap.add_argument(
        "--skip-build",
        action="store_true",
        help="fail instead of building Perry when --perry-bin is missing",
    )
    return ap.parse_args()


def main() -> int:
    args = parse_args()
    root = args.root.resolve()
    perry_bin = args.perry_bin.resolve()
    reports_dir = args.reports_dir.resolve()

    if args.skip_vendor:
        if not ((root / "test").is_dir() and (root / "harness").is_dir()):
            raise SystemExit(f"error: Test262 checkout missing at {root}")
    else:
        ensure_test262(root)

    if args.skip_build:
        if not perry_bin.exists():
            raise SystemExit(f"error: Perry binary missing at {perry_bin}")
    else:
        ensure_perry(perry_bin)

    reports_dir.mkdir(parents=True, exist_ok=True)
    stem = report_stem(args.dir, args.max)
    json_report = reports_dir / f"{stem}.json"
    tsv_report = reports_dir / f"{stem}-problems.tsv"

    cmd = [
        sys.executable,
        str(SCRIPT_DIR / "test262_subset.py"),
        "--root",
        str(root),
        "--dir",
        *args.dir,
        "--max",
        str(args.max),
        "--sample-cap",
        str(args.sample_cap),
        "--timeout",
        str(args.timeout),
        "--perry-bin",
        str(perry_bin),
        "--report",
        str(json_report),
    ]
    run_command(cmd)

    problem_count = write_problem_tsv(json_report, tsv_report)
    print(f"json report: {json_report}")
    print(f"problem tsv: {tsv_report} ({problem_count} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
