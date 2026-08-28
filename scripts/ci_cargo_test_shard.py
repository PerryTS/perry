#!/usr/bin/env python3
"""Deterministically shard a Cargo package's integration-test targets.

The full CI tier runs the large `perry` integration-test inventory in a
matrix.  This helper derives the inventory from `cargo metadata` instead of a
hand-maintained list, then assigns the sorted target names round-robin across
one-based shards.  Every integration target therefore belongs to exactly one
shard, including a target added by the same commit that changes the workflow.

Examples:
  python3 scripts/ci_cargo_test_shard.py --package perry --shard 1 --total-shards 8
  python3 scripts/ci_cargo_test_shard.py --package perry --total-shards 8 --validate
  python3 scripts/ci_cargo_test_shard.py --self-test
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
import subprocess
import sys


def _load_metadata() -> dict:
    raw = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"]
    )
    return json.loads(raw)


def integration_targets(metadata: dict, package_name: str) -> list[str]:
    """Return every integration-test target for one workspace package."""
    packages = [
        package for package in metadata["packages"] if package["name"] == package_name
    ]
    if len(packages) != 1:
        raise ValueError(
            f"expected exactly one package named {package_name!r}, found {len(packages)}"
        )

    targets = sorted(
        target["name"]
        for target in packages[0]["targets"]
        if "test" in target.get("kind", [])
    )
    if len(targets) != len(set(targets)):
        duplicates = sorted(
            name for name, count in Counter(targets).items() if count > 1
        )
        raise ValueError(f"duplicate integration-test targets: {', '.join(duplicates)}")
    if not targets:
        raise ValueError(f"package {package_name!r} has no integration-test targets")
    return targets


def shard_targets(targets: list[str], shard: int, total_shards: int) -> list[str]:
    """Return a stable, count-balanced one-based round-robin partition."""
    if total_shards < 1:
        raise ValueError("total shards must be at least 1")
    if not 1 <= shard <= total_shards:
        raise ValueError(f"shard must be between 1 and {total_shards}, got {shard}")
    return sorted(targets)[shard - 1 :: total_shards]


def validate_assignments(
    targets: list[str], total_shards: int, package_name: str
) -> None:
    if total_shards < 1:
        raise ValueError("total shards must be at least 1")
    assignments = [
        shard_targets(targets, shard, total_shards)
        for shard in range(1, total_shards + 1)
    ]
    empty = [index + 1 for index, assigned in enumerate(assignments) if not assigned]
    if empty:
        raise ValueError(f"empty shards: {', '.join(map(str, empty))}")

    assigned_counts = Counter(name for assigned in assignments for name in assigned)
    expected_counts = Counter(targets)
    if assigned_counts != expected_counts:
        missing = sorted((expected_counts - assigned_counts).elements())
        repeated = sorted((assigned_counts - expected_counts).elements())
        raise ValueError(
            f"invalid coverage: missing={missing or 'none'}, repeated={repeated or 'none'}"
        )

    sizes = [len(assigned) for assigned in assignments]
    if max(sizes) - min(sizes) > 1:
        raise ValueError(f"unbalanced shard sizes: {sizes}")
    print(
        f"{package_name}: {len(targets)} integration targets assigned exactly once "
        f"across {total_shards} shards (sizes: {', '.join(map(str, sizes))})"
    )


def _self_test() -> int:
    metadata = {
        "packages": [
            {
                "name": "perry",
                "targets": [
                    {"name": "z_suite", "kind": ["test"]},
                    {"name": "perry", "kind": ["bin"]},
                    {"name": "a_suite", "kind": ["test"]},
                ],
            }
        ]
    }
    if integration_targets(metadata, "perry") != ["a_suite", "z_suite"]:
        print("Cargo metadata target discovery drifted", file=sys.stderr)
        return 1

    sample = [f"test_{index:02d}" for index in range(19)]
    expected = [
        ["test_00", "test_04", "test_08", "test_12", "test_16"],
        ["test_01", "test_05", "test_09", "test_13", "test_17"],
        ["test_02", "test_06", "test_10", "test_14", "test_18"],
        ["test_03", "test_07", "test_11", "test_15"],
    ]
    actual = [shard_targets(list(reversed(sample)), shard, 4) for shard in range(1, 5)]
    if actual != expected:
        print(f"round-robin assignment drifted: {actual!r}", file=sys.stderr)
        return 1

    flattened = [name for assigned in actual for name in assigned]
    if Counter(flattened) != Counter(sample):
        print("self-test assignment did not cover every target exactly once", file=sys.stderr)
        return 1

    for shard, total in ((0, 4), (5, 4), (1, 0)):
        try:
            shard_targets(sample, shard, total)
        except ValueError:
            pass
        else:
            print(
                f"invalid shard {shard}/{total} was accepted",
                file=sys.stderr,
            )
            return 1

    print("ci_cargo_test_shard --self-test: deterministic exact coverage holds")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", default="perry")
    parser.add_argument("--shard", type=int)
    parser.add_argument("--total-shards", type=int)
    parser.add_argument("--validate", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return _self_test()
    if args.total_shards is None:
        parser.error("--total-shards is required")
    if args.validate and args.shard is not None:
        parser.error("--validate and --shard are mutually exclusive")
    if not args.validate and args.shard is None:
        parser.error("--shard is required unless --validate is used")

    try:
        targets = integration_targets(_load_metadata(), args.package)
        if args.validate:
            validate_assignments(targets, args.total_shards, args.package)
        else:
            for target in shard_targets(targets, args.shard, args.total_shards):
                print(target)
    except (KeyError, TypeError, ValueError) as exc:
        parser.error(str(exc))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
