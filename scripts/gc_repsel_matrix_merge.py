#!/usr/bin/env python3
"""Strict fan-in for sharded gc_repsel_matrix.sh reports.

The matrix is sharded by corpus file, so no individual shard is allowed to
make a corpus-wide liveness claim. This merger proves that every deterministic
slice arrived, that every expected test/arm cell appears exactly once, and then
runs the existing liveness gate over the reconstructed whole-suite report.

Usage:
  scripts/gc_repsel_matrix_merge.py --expect 4 --mode all \
      --output gc-repsel-matrix.json shard-*/gc-repsel-matrix.json
  scripts/gc_repsel_matrix_merge.py --self-test
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

from gc_matrix_liveness_check import (
    MATRIX,
    REGISTRY,
    Violation,
    check_report,
    matrix_arms,
    matrix_pr_arms,
    parse_registry,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "test-parity" / "gc_repsel_corpus.txt"
RESULTS = ("PASS", "UNVER", "XFAIL", "FAIL")


def corpus_from_manifest(text: str) -> list[str]:
    corpus = []
    for raw in text.splitlines():
        value = raw.split("#", 1)[0].strip()
        if value:
            corpus.append("".join(value.split()))
    if not corpus:
        raise Violation(f"{MANIFEST.name}: corpus is empty")
    return corpus


def expected_arms(mode: str, matrix_text: str) -> list[dict[str, str]]:
    declared = matrix_arms(matrix_text)
    ids = list(declared) if mode == "all" else matrix_pr_arms(matrix_text)
    return [{"id": arm_id, "requires": declared[arm_id]} for arm_id in ids]


def load(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            report = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        raise Violation(f"{path}: cannot read a complete matrix report: {exc}") from exc
    if not isinstance(report, dict):
        raise Violation(f"{path}: matrix report root must be an object")
    return report


def merge(
    reports: list[dict[str, Any]],
    sources: list[str],
    expect: int,
    arms: list[dict[str, str]],
    corpus: list[str],
) -> dict[str, Any]:
    if len(reports) != expect:
        raise Violation(
            f"expected exactly {expect} shard reports, got {len(reports)} — "
            "a missing artifact must fail the matrix, not shrink it"
        )

    arm_ids = [arm["id"] for arm in arms]
    expected_arm_set = set(arm_ids)
    expected_by_shard = {
        shard: corpus[shard - 1 :: expect] for shard in range(1, expect + 1)
    }
    by_shard: dict[int, tuple[dict[str, Any], str]] = {}
    node = None
    pressure = None

    for report, source in zip(reports, sources):
        if report.get("complete") is not True:
            raise Violation(f"{source}: report is not marked complete")
        shard = report.get("shard")
        if not isinstance(shard, dict):
            raise Violation(f"{source}: missing shard metadata")
        index, count = shard.get("index"), shard.get("count")
        if not isinstance(index, int) or not isinstance(count, int):
            raise Violation(f"{source}: shard index/count must be integers")
        if count != expect or index not in expected_by_shard:
            raise Violation(
                f"{source}: declares shard {index}/{count}, expected one of 1/{expect}..{expect}/{expect}"
            )
        if index in by_shard:
            raise Violation(
                f"shard {index}/{expect} appears in both {by_shard[index][1]} and {source}"
            )
        by_shard[index] = (report, source)

        if shard.get("corpus_total") != len(corpus):
            raise Violation(
                f"{source}: corpus_total={shard.get('corpus_total')!r}, expected {len(corpus)}"
            )
        if shard.get("corpus_selected") != len(expected_by_shard[index]):
            raise Violation(
                f"{source}: corpus_selected={shard.get('corpus_selected')!r}, "
                f"expected {len(expected_by_shard[index])} for shard {index}/{expect}"
            )
        if report.get("arms") != arms:
            raise Violation(
                f"{source}: arm inventory/order does not match the current matrix's expected selection"
            )
        if node is None:
            node, pressure = report.get("node"), report.get("pressure_mb")
        elif report.get("node") != node or report.get("pressure_mb") != pressure:
            raise Violation(f"{source}: node/pressure configuration differs across shards")

    missing_shards = sorted(set(expected_by_shard) - set(by_shard))
    if missing_shards:
        raise Violation(f"missing shard reports: {missing_shards}")

    merged_cells: list[dict[str, Any]] = []
    for shard_index in range(1, expect + 1):
        report, source = by_shard[shard_index]
        cells = report.get("cells")
        if not isinstance(cells, list):
            raise Violation(f"{source}: cells must be an array")
        wanted_tests = set(expected_by_shard[shard_index])
        # The historical manifest currently repeats three witnesses. Preserve
        # that exact workload: cells are a multiset, not a set, and each listed
        # occurrence must arrive once from its deterministic shard.
        wanted_keys = Counter(
            (test, arm)
            for test in expected_by_shard[shard_index]
            for arm in arm_ids
        )
        found_keys: Counter[tuple[str, str]] = Counter()
        for cell in cells:
            if not isinstance(cell, dict):
                raise Violation(f"{source}: every cells[] entry must be an object")
            test, arm, result = cell.get("test"), cell.get("arm"), cell.get("result")
            key = (test, arm)
            if test not in wanted_tests:
                raise Violation(
                    f"{source}: test {test!r} does not belong to deterministic shard {shard_index}/{expect}"
                )
            if arm not in expected_arm_set:
                raise Violation(f"{source}: unexpected arm {arm!r}")
            if result not in RESULTS:
                raise Violation(f"{source}: invalid result {result!r} for {test}/{arm}")
            found_keys[key] += 1
            merged_cells.append(cell)
        missing_cells = sorted((wanted_keys - found_keys).elements())
        extra_cells = sorted((found_keys - wanted_keys).elements())
        if missing_cells or extra_cells:
            detail = []
            if missing_cells:
                detail.append(f"missing {len(missing_cells)} cells (first: {missing_cells[:3]})")
            if extra_cells:
                detail.append(f"unexpected {len(extra_cells)} cells (first: {extra_cells[:3]})")
            raise Violation(f"{source}: incomplete shard coverage: {'; '.join(detail)}")

        counts = Counter(cell["result"] for cell in cells)
        declared_summary = report.get("summary")
        expected_summary = {
            "pass": counts["PASS"],
            "unverified": counts["UNVER"],
            "xfail": counts["XFAIL"],
            "fail": counts["FAIL"],
        }
        if declared_summary != expected_summary:
            raise Violation(
                f"{source}: summary {declared_summary!r} does not match its cells {expected_summary!r}"
            )

    expected_cells = len(corpus) * len(arms)
    if len(merged_cells) != expected_cells:
        raise Violation(f"merged {len(merged_cells)} cells, expected {expected_cells}")
    expected_keys = Counter((test, arm) for test in corpus for arm in arm_ids)
    merged_keys = Counter((cell["test"], cell["arm"]) for cell in merged_cells)
    if merged_keys != expected_keys:
        raise Violation("merged cell multiset does not match the manifest x arm cross-product")

    counts = Counter(cell["result"] for cell in merged_cells)
    return {
        "node": node,
        "pressure_mb": pressure,
        "complete": True,
        "merged_from": expect,
        "arms": arms,
        "cells": merged_cells,
        "summary": {
            "pass": counts["PASS"],
            "unverified": counts["UNVER"],
            "xfail": counts["XFAIL"],
            "fail": counts["FAIL"],
        },
    }


def _fixture_report(
    shard: int,
    total: int,
    corpus: list[str],
    arms: list[dict[str, str]],
) -> dict[str, Any]:
    tests = corpus[shard - 1 :: total]
    cells = []
    for test in tests:
        for arm in arms:
            cells.append(
                {
                    "test": test,
                    "arm": arm["id"],
                    "result": "PASS",
                    "cycles": 1,
                    "evacuated": 1 if arm["requires"] == "move" else 0,
                    "scavenged": 0,
                    "reclaimed": 0,
                    "evidence": "fixture",
                }
            )
    return {
        "node": "26.5.1",
        "pressure_mb": "8",
        "complete": True,
        "shard": {
            "index": shard,
            "count": total,
            "corpus_total": len(corpus),
            "corpus_selected": len(tests),
        },
        "arms": arms,
        "cells": cells,
        "summary": {"pass": len(cells), "unverified": 0, "xfail": 0, "fail": 0},
    }


def self_test() -> int:
    failures = []
    # `a` is intentionally repeated, matching the real manifest's historical
    # repeated witnesses and exercising multiset coverage across shards.
    corpus = ["a", "b", "c", "a", "d"]
    arms = [{"id": "move", "requires": "move"}, {"id": "control", "requires": "none"}]
    good = [_fixture_report(1, 2, corpus, arms), _fixture_report(2, 2, corpus, arms)]

    def expect(name: str, reports: list[dict[str, Any]], should_fail: bool) -> None:
        try:
            merge(reports, [f"s{i + 1}" for i in range(len(reports))], 2, arms, corpus)
            failed = False
        except Violation:
            failed = True
        if failed != should_fail:
            failures.append(name)

    expect("complete two-shard merge", good, False)
    expect("missing shard rejected", good[:1], True)
    duplicate = [json.loads(json.dumps(good[0])), json.loads(json.dumps(good[0]))]
    expect("duplicate shard rejected", duplicate, True)
    incomplete = json.loads(json.dumps(good))
    incomplete[0]["complete"] = False
    expect("incomplete report rejected", incomplete, True)
    missing_cell = json.loads(json.dumps(good))
    missing_cell[0]["cells"].pop()
    expect("missing cell rejected", missing_cell, True)
    wrong_slice = json.loads(json.dumps(good))
    wrong_slice[0]["cells"][0]["test"] = "b"
    expect("wrong deterministic slice rejected", wrong_slice, True)
    bad_summary = json.loads(json.dumps(good))
    bad_summary[0]["summary"]["pass"] -= 1
    expect("dishonest summary rejected", bad_summary, True)

    if failures:
        print("gc_repsel_matrix_merge --self-test FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("gc_repsel_matrix_merge --self-test: OK (7 cases)")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("inputs", nargs="*", type=Path)
    parser.add_argument("--expect", type=int)
    parser.add_argument("--mode", choices=("pr", "all"))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()
    if not args.expect or not args.mode or not args.output or not args.inputs:
        parser.error("--expect, --mode, --output and at least one input are required")

    try:
        arms = expected_arms(args.mode, MATRIX.read_text(encoding="utf-8"))
        corpus = corpus_from_manifest(MANIFEST.read_text(encoding="utf-8"))
        reports = [load(path) for path in args.inputs]
        merged = merge(reports, [str(path) for path in args.inputs], args.expect, arms, corpus)
        registry = parse_registry(REGISTRY.read_text(encoding="utf-8")) if REGISTRY.exists() else {}
        liveness_violations = check_report(merged, registry)
    except Violation as exc:
        print(f"GC MATRIX FAN-IN: {exc}", file=sys.stderr)
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as handle:
        json.dump(merged, handle, indent=1)
        handle.write("\n")

    for violation in liveness_violations:
        print(f"LIVENESS GATE: {violation}", file=sys.stderr)
    summary = merged["summary"]
    print(
        f"merged {args.expect} complete shard report(s) -> {args.output}: "
        f"PASS={summary['pass']} UNVER={summary['unverified']} "
        f"XFAIL={summary['xfail']} FAIL={summary['fail']}"
    )
    if liveness_violations or summary["fail"]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
