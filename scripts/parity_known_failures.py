#!/usr/bin/env python3
"""Ratchet for the platform-aware parity known-failure allowlist.

`test-parity/known_failures.json` used to be a pure SUPPRESSION list: this
script computed `failures - allowed` and stopped there, so an entry whose test
had started passing was inert forever. It never failed, never reported, and
never asked to be removed. That is a fifth way a gate can be unable to fail
(CLAUDE.md names four) and the most insidious, because the job is genuinely
green AND genuinely running — while the entry silently converts any future
regression of that exact test into a non-event.

It cost a real bug. `test_gap_diagchannel_3082_3084_3085_3086` was listed on
2026-07-04 for a feature cluster that shipped; the entry stayed. When #7105
broke the test again for an unrelated reason (`PreallocateBoxes` shadowing a
module-level global), the suppression absorbed it. The defect emptied every
`let`/`const` in a top-level bare block of an ES module that a sibling
`function` declaration read — six days, found by accident (#7580).

So the check now runs in BOTH directions, the way
`scripts/gap_root_dominance_allowlist.json` and `test-parity/gap_snapshot.json`
already do in this repo (#7582):

  * fails and is NOT allowed here   -> regression. Fix it, or triage it.
  * allowed here and now PASSES     -> STALE. Delete the entry, in this PR.

"Now passes" is a per-platform verdict about the platform the run executed on,
and it needs POSITIVE evidence: the test must appear in the report's `results[]`
with status `pass`. An entry whose test did not run at all — filtered out, not
in this shard, `node_fail`, skipped — is never flagged. Absence of evidence is
not passing; that is exactly the hole a Node-22 pin used to hide 14 tests in
(#6364).

Two extra teeth, both cheap:

  * `--audit` needs no report and no test run. It validates provenance (#797:
    every entry carries an issue and the date it was listed) and cross-checks
    the gap-suite entries against the committed `gap_snapshot.json` — a
    generated, bidirectional baseline. A `test_gap_*` entry ABSENT from that
    snapshot is one the snapshot asserts passes, i.e. stale. This matters
    because the `parity` job that consumes this file is TAG-gated: without an
    offline check the ratchet would almost never fire before a merge. Run from
    `lint`, which is a required context. It would have flagged the diagchannel
    entry on the day it was added.

  * the report path prints how many entries it actually ADJUDICATED. A gate
    must assert its subject was live, not merely that nothing threw.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPORT = ROOT / "test-parity" / "reports" / "latest.json"
DEFAULT_KNOWN = ROOT / "test-parity" / "known_failures.json"
DEFAULT_GAP_SNAPSHOT = ROOT / "test-parity" / "gap_snapshot.json"
DEFAULT_TEST_DIR = ROOT / "test-files"
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
PASS = "pass"
# Mirrors test-parity/README.md ("Category definitions"). An undocumented
# category is a schema error: `toolchain` slipped in unnoticed precisely
# because nothing validated this field.
CATEGORIES = frozenset(
    {
        "ci-env",
        "module-inventory",
        "bug-open",
        "bug-stale",
        "gap-categorical",
        "gap-bisect",
        "untriaged",
    }
)
ISSUE_RE = re.compile(r"^[1-9][0-9]*$")
DATE_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")
# The snapshot cross-check only speaks for the suite the snapshot covers.
GAP_PREFIX = "test_gap_"
# gap_snapshot.json is generated on Linux and is the baseline required CI uses.
GAP_SNAPSHOT_PLATFORM = "linux"


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


def report_statuses(report: dict) -> dict[str, str]:
    """test id -> status, from `results[]`.

    `results[]` is REQUIRED, not best-effort. `failures{}` alone can only ever
    say what failed; the stale half of the ratchet needs to know what RAN and
    passed. Silently degrading to "no results, nothing is stale" would make
    this gate unable to fail again, which is the whole bug being fixed.
    """
    results = report.get("results")
    if not isinstance(results, list) or not results:
        raise ValueError(
            "parity report must contain a non-empty results[] array — the "
            "stale-entry check cannot tell 'passed' from 'never ran' without it"
        )
    statuses: dict[str, str] = {}
    for item in results:
        if not isinstance(item, dict):
            raise ValueError(f"malformed results[] entry: {item!r}")
        test_id, status = item.get("id"), item.get("status")
        if not isinstance(test_id, str) or not isinstance(status, str):
            raise ValueError(f"malformed results[] entry: {item!r}")
        statuses[test_id] = status
    return statuses


def validate_entry(test_id: str, record: object) -> list[str]:
    """Schema + provenance rules for one entry (#797)."""
    problems: list[str] = []
    if not isinstance(record, dict):
        return [f"{test_id}: entry must be an object"]

    category = record.get("category")
    if not isinstance(category, str) or not category:
        problems.append(f"{test_id}: category must be a non-empty string")
    elif category not in CATEGORIES:
        problems.append(
            f"{test_id}: category {category!r} is not one of "
            f"{', '.join(sorted(CATEGORIES))} (see test-parity/README.md)"
        )
    if not isinstance(record.get("reason"), str) or not record["reason"]:
        problems.append(f"{test_id}: reason must be a non-empty string")

    # #797: provenance is mandatory. An entry with no issue and no date cannot
    # be revalidated by anyone but its author, and that is how the pre-audit
    # bare-name format decayed into orphans.
    issue = record.get("issue")
    if not isinstance(issue, str) or not ISSUE_RE.match(issue):
        problems.append(
            f"{test_id}: issue must be a GitHub issue number as a string "
            f'(e.g. "793"); got {issue!r}'
        )
    added = record.get("added")
    if not isinstance(added, str) or not DATE_RE.match(added):
        problems.append(f"{test_id}: added must be an ISO date YYYY-MM-DD; got {added!r}")

    platforms = record.get("platforms")
    if platforms is not None:
        if (
            not isinstance(platforms, list)
            or not platforms
            or not all(isinstance(item, str) for item in platforms)
        ):
            problems.append(f"{test_id}: platforms must be a non-empty string array")
        else:
            unknown = sorted(
                {item for item in platforms if item.strip().lower() not in PLATFORM_ALIASES}
            )
            if unknown:
                problems.append(f"{test_id}: unknown platforms: {', '.join(unknown)}")
            elif len({normalize_platform(item) for item in platforms}) != len(platforms):
                problems.append(f"{test_id}: platforms must not contain duplicates")
    return problems


def entry_applies(record: object, platform: str) -> bool:
    """Does this entry claim to cover `platform`?

    An entry scoped to other platforms says nothing about this host, so it is
    neither honoured as a suppression nor judged stale here.
    """
    if not isinstance(record, dict):
        return False
    platforms = record.get("platforms")
    if platforms is None:
        return True
    if not isinstance(platforms, list):
        return False
    return platform in {
        normalize_platform(item) for item in platforms if isinstance(item, str)
    }


def known_for_platform(known: dict, platform: str) -> tuple[set[str], list[str]]:
    selected: set[str] = set()
    problems: list[str] = []
    for test_id, record in known.items():
        if test_id == "_schema":
            continue
        entry_problems = validate_entry(test_id, record)
        problems.extend(entry_problems)
        # A malformed `platforms` value cannot be trusted to scope the entry.
        if any("platforms" in problem for problem in entry_problems):
            continue
        if entry_applies(record, platform):
            selected.add(test_id)
    return selected, problems


def stale_entries(allowed: set[str], statuses: dict[str, str]) -> list[str]:
    """Allowed-here entries whose test RAN on this platform and PASSED."""
    return sorted(t for t in allowed if statuses.get(t) == PASS)


def check(
    report: dict, known: dict, platform_override: str | None = None
) -> tuple[str, list[str], list[str], list[str], int]:
    platform_value = platform_override or report.get("platform") or sys.platform
    if not isinstance(platform_value, str):
        raise ValueError("report platform must be a string")
    platform = normalize_platform(platform_value)
    failures = report_failures(report)
    statuses = report_statuses(report)
    allowed, schema_problems = known_for_platform(known, platform)
    stale = stale_entries(allowed, statuses)
    adjudicated = sum(1 for test_id in allowed if test_id in statuses)
    return platform, sorted(failures - allowed), stale, schema_problems, adjudicated


def audit(
    known: dict,
    snapshot_tests: dict | None,
    test_exists=None,
) -> tuple[list[str], list[str]]:
    """Offline half: provenance + cross-check against the gap snapshot.

    Returns (schema_problems, stale_problems). Needs no parity run, so it can
    live on a required per-PR job while the suite that consumes this file is
    tag-gated.
    """
    if test_exists is None:
        def test_exists(test_id: str) -> bool:
            return (DEFAULT_TEST_DIR / f"{test_id}.ts").exists()

    _, schema_problems = known_for_platform(known, GAP_SNAPSHOT_PLATFORM)
    stale: list[str] = []
    for test_id, record in sorted(known.items()):
        if test_id == "_schema":
            continue
        if not test_exists(test_id):
            stale.append(
                f"{test_id}: no test-files/{test_id}.ts — the test this entry "
                f"suppresses no longer exists"
            )
            continue
        if snapshot_tests is None or not test_id.startswith(GAP_PREFIX):
            continue
        if not entry_applies(record, GAP_SNAPSHOT_PLATFORM):
            continue
        if test_id not in snapshot_tests:
            stale.append(
                f"{test_id}: test-parity/gap_snapshot.json (the generated "
                f"{GAP_SNAPSHOT_PLATFORM} baseline) says this test PASSES"
            )
    return schema_problems, stale


def print_stale(stale: list[str], platform: str | None = None) -> None:
    where = f" on {platform}" if platform else ""
    print(
        f"\nSTALE known_failures.json entries — these tests PASS{where}:",
        file=sys.stderr,
    )
    for item in stale:
        print(f"  - {item}", file=sys.stderr)
    print(
        "\nDelete each one from test-parity/known_failures.json, in the SAME PR.\n"
        "An entry that outlives its bug is not a triage note, it is a permanent\n"
        "suppression of that test's next regression (#7582).",
        file=sys.stderr,
    )


def self_test() -> int:
    report = {
        "platform": "windows",
        "failures": {
            "parity": ["all_hosts", "windows_only", "linux_only"],
            "compile": [""],
            "crash": ["windows_crash"],
        },
        "results": [
            {"id": "all_hosts", "status": "parity_fail"},
            {"id": "windows_only", "status": "parity_fail"},
            {"id": "linux_only", "status": "parity_fail"},
            {"id": "windows_crash", "status": "crash"},
        ],
    }

    def entry(**overrides) -> dict:
        base = {
            "issue": "793",
            "added": "2026-05-15",
            "category": "bug-open",
            "reason": "why",
        }
        base.update(overrides)
        return base

    known = {
        "_schema": {},
        "all_hosts": entry(),
        "windows_only": entry(platforms=["win32"]),
        "windows_crash": entry(platforms=["msys"]),
        "linux_only": entry(platforms=["linux"]),
    }
    platform, new, stale, problems, adjudicated = check(report, known)
    assert platform == "windows"
    assert new == ["linux_only"]
    assert stale == []
    assert problems == []
    # linux_only is not selected on windows, so it is not adjudicated here.
    assert adjudicated == 3, adjudicated
    assert normalize_platform("darwin") == "macos"

    # --- the ratchet: an allowed entry whose test now PASSES is stale --------
    fixed = dict(report)
    fixed["failures"] = {"parity": ["linux_only"], "compile": [], "crash": []}
    fixed["results"] = [
        {"id": "all_hosts", "status": PASS},
        {"id": "windows_only", "status": "parity_fail"},
        {"id": "windows_crash", "status": "crash"},
        {"id": "linux_only", "status": "parity_fail"},
    ]
    _, _, stale, _, _ = check(fixed, known)
    assert stale == ["all_hosts"], stale

    # A test that did NOT run is never stale — absence of evidence is not a pass.
    absent = dict(fixed)
    absent["results"] = [{"id": "windows_only", "status": "parity_fail"}]
    _, _, stale, _, adjudicated = check(absent, known)
    assert stale == [], stale
    assert adjudicated == 1, adjudicated

    # node_fail / skipped are not passes either.
    for status in ("node_fail", "skipped", "compile_fail"):
        probe = dict(fixed)
        probe["results"] = [{"id": "all_hosts", "status": status}]
        _, _, stale, _, _ = check(probe, known)
        assert stale == [], (status, stale)

    # An entry scoped to another platform is not judged here, even if it passes.
    other = dict(fixed)
    other["results"] = [{"id": "linux_only", "status": PASS}]
    _, _, stale, _, _ = check(other, known)
    assert stale == [], stale

    # results[] is mandatory: no silent degradation to a suppression-only gate.
    try:
        check({"platform": "linux", "failures": {"parity": [], "compile": [], "crash": []}}, known)
    except ValueError as error:
        assert "results[]" in str(error), error
    else:  # pragma: no cover - guarded by the assert below
        raise AssertionError("a report with no results[] must be rejected")

    # --- schema / provenance -------------------------------------------------
    malformed = {
        "bad": {"category": "", "reason": "", "platforms": ["plan9"], "issue": "", "added": "x"},
    }
    _, _, _, problems, _ = check(report, malformed)
    assert len(problems) == 5, problems
    undocumented = {"bad": entry(category="toolchain")}
    _, _, _, problems, _ = check(report, undocumented)
    assert len(problems) == 1 and "not one of" in problems[0], problems
    for field, value in (("issue", None), ("issue", 793), ("added", "2026-5-15")):
        _, _, _, problems, _ = check(report, {"bad": entry(**{field: value})})
        assert len(problems) == 1 and field in problems[0], (field, value, problems)

    # --- offline audit -------------------------------------------------------
    gap_known = {
        "test_gap_still_broken": entry(),
        "test_gap_fixed": entry(),
        "test_gap_linux_scoped_elsewhere": entry(platforms=["windows"]),
        "test_parity_other_suite": entry(),
    }
    snapshot = {"test_gap_still_broken": {"status": "parity_fail"}}
    problems, stale = audit(gap_known, snapshot, test_exists=lambda _t: True)
    assert problems == [], problems
    assert len(stale) == 1 and stale[0].startswith("test_gap_fixed:"), stale
    # A vanished test file is stale whatever the snapshot says.
    _, stale = audit(
        {"test_gap_gone": entry()}, snapshot, test_exists=lambda _t: False
    )
    assert len(stale) == 1 and "no longer exists" in stale[0], stale
    # No snapshot -> the file-existence half still runs, the cross-check does not.
    _, stale = audit(gap_known, None, test_exists=lambda _t: True)
    assert stale == [], stale

    print("parity_known_failures self-test OK")
    return 0


def run_audit(args: argparse.Namespace) -> int:
    try:
        known = load_json(args.known) if args.known.exists() else {}
        snapshot_tests = None
        if args.gap_snapshot.exists():
            snapshot_tests = load_json(args.gap_snapshot).get("tests", {})
        problems, stale = audit(known, snapshot_tests)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"known-failure audit error: {error}", file=sys.stderr)
        return 2

    if problems:
        print("Malformed known_failures.json entries:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 2
    if stale:
        print_stale(stale)
        return 1

    entries = sum(1 for key in known if key != "_schema")
    scope = (
        f" and cross-checked {sum(1 for k in known if k.startswith(GAP_PREFIX))} "
        f"gap entries against {args.gap_snapshot.name}"
        if snapshot_tests is not None
        else " (no gap snapshot found — cross-check skipped)"
    )
    print(f"known_failures.json audit OK — {entries} entries carry provenance{scope}.")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--known", type=Path, default=DEFAULT_KNOWN)
    parser.add_argument("--gap-snapshot", type=Path, default=DEFAULT_GAP_SNAPSHOT)
    parser.add_argument("--platform")
    parser.add_argument(
        "--audit",
        action="store_true",
        help="offline provenance + gap-snapshot cross-check; needs no parity run",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()
    if args.audit:
        return run_audit(args)

    try:
        report = load_json(args.report)
        known = load_json(args.known) if args.known.exists() else {}
        platform, new_failures, stale, schema_problems, adjudicated = check(
            report, known, args.platform
        )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"known-failure check error: {error}", file=sys.stderr)
        return 2

    if schema_problems:
        print("Malformed known_failures.json entries:", file=sys.stderr)
        for problem in schema_problems:
            print(f"  - {problem}", file=sys.stderr)
        return 2

    status = 0
    if new_failures:
        print(
            f"NEW FAILURES on {platform} (not allowed for this platform):",
            file=sys.stderr,
        )
        for test_id in new_failures:
            print(f"  - {test_id}", file=sys.stderr)
        status = 1
    if stale:
        print_stale(stale, platform)
        status = 1
    if status:
        return status

    total = len(report_failures(report))
    allowed, _ = known_for_platform(known, platform)
    print(
        f"All {total} failures are known/triaged for {platform}; "
        f"{adjudicated}/{len(allowed)} allowlist entries were adjudicated by this "
        f"run (the rest did not run here)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
