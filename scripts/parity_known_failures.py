#!/usr/bin/env python3
"""Check a parity report against the platform-aware known-failure allowlist."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPORT = ROOT / "test-parity" / "reports" / "latest.json"
DEFAULT_KNOWN = ROOT / "test-parity" / "known_failures.json"
PLATFORM_ALIASES = {
    "cygwin": "windows",
    "darwin": "macos",
    "linux": "linux",
    "macos": "macos",
    "mingw": "windows",
    "msys": "windows",
    "other": "other",
    "win32": "windows",
    "windows": "windows",
}
PLATFORMS = frozenset(PLATFORM_ALIASES.values())


def normalize_platform(value: str) -> str:
    folded = value.strip().lower()
    return PLATFORM_ALIASES.get(folded, "other")


def load_json(path: Path) -> dict:
    with path.open(encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return data


def report_failures(report: dict) -> set[str]:
    failures = report.get("failures")
    if not isinstance(failures, dict):
        raise ValueError("parity report must contain a failures object")
    result: set[str] = set()
    for category in ("parity", "compile", "crash"):
        values = failures.get(category) or []
        if not isinstance(values, list) or not all(isinstance(item, str) for item in values):
            raise ValueError(f"failures.{category} must be a string array")
        result.update(item for item in values if item)
    return result


def known_for_platform(known: dict, platform: str) -> tuple[set[str], list[str]]:
    selected: set[str] = set()
    problems: list[str] = []
    for test_id, record in known.items():
        if test_id == "_schema":
            continue
        if not isinstance(record, dict):
            problems.append(f"{test_id}: entry must be an object")
            continue
        if not isinstance(record.get("category"), str) or not record["category"]:
            problems.append(f"{test_id}: category must be a non-empty string")
        if not isinstance(record.get("reason"), str) or not record["reason"]:
            problems.append(f"{test_id}: reason must be a non-empty string")

        platforms = record.get("platforms")
        if platforms is None:
            selected.add(test_id)
            continue
        if (
            not isinstance(platforms, list)
            or not platforms
            or not all(isinstance(item, str) for item in platforms)
        ):
            problems.append(f"{test_id}: platforms must be a non-empty string array")
            continue
        normalized = [normalize_platform(item) for item in platforms]
        unknown = sorted(
            {item for item in platforms if item.strip().lower() not in PLATFORM_ALIASES}
        )
        if unknown:
            problems.append(f"{test_id}: unknown platforms: {', '.join(unknown)}")
            continue
        if len(normalized) != len(set(normalized)):
            problems.append(f"{test_id}: platforms must not contain duplicates")
            continue
        if platform in normalized:
            selected.add(test_id)
    return selected, problems


def check(report: dict, known: dict, platform_override: str | None = None) -> tuple[str, list[str], list[str]]:
    platform_value = platform_override or report.get("platform") or sys.platform
    if not isinstance(platform_value, str):
        raise ValueError("report platform must be a string")
    platform = normalize_platform(platform_value)
    failures = report_failures(report)
    allowed, schema_problems = known_for_platform(known, platform)
    return platform, sorted(failures - allowed), schema_problems


def self_test() -> int:
    report = {
        "platform": "windows",
        "failures": {
            "parity": ["all_hosts", "windows_only", "linux_only"],
            "compile": [""],
            "crash": ["windows_crash"],
        },
    }
    known = {
        "_schema": {},
        "all_hosts": {"category": "bug-open", "reason": "all"},
        "windows_only": {
            "category": "bug-open",
            "reason": "win",
            "platforms": ["win32"],
        },
        "windows_crash": {
            "category": "bug-open",
            "reason": "crash",
            "platforms": ["msys"],
        },
        "linux_only": {
            "category": "bug-open",
            "reason": "linux",
            "platforms": ["linux"],
        },
    }
    platform, new, problems = check(report, known)
    assert platform == "windows"
    assert new == ["linux_only"]
    assert problems == []
    assert normalize_platform("darwin") == "macos"

    malformed = {
        "bad": {"category": "", "reason": "", "platforms": ["plan9"]},
    }
    _, _, problems = check(report, malformed)
    assert len(problems) == 3, problems
    print("parity_known_failures self-test OK")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--known", type=Path, default=DEFAULT_KNOWN)
    parser.add_argument("--platform")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    try:
        report = load_json(args.report)
        known = load_json(args.known) if args.known.exists() else {}
        platform, new_failures, schema_problems = check(report, known, args.platform)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"known-failure check error: {error}", file=sys.stderr)
        return 2

    if schema_problems:
        print("Malformed known_failures.json entries:", file=sys.stderr)
        for problem in schema_problems:
            print(f"  - {problem}", file=sys.stderr)
        return 2
    if new_failures:
        print(
            f"NEW FAILURES on {platform} (not allowed for this platform):",
            file=sys.stderr,
        )
        for test_id in new_failures:
            print(f"  - {test_id}", file=sys.stderr)
        return 1

    total = len(report_failures(report))
    print(f"All {total} failures are known/triaged for {platform}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
