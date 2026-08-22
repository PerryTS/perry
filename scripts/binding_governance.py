#!/usr/bin/env python3
"""Validate and render the bundled native-binding governance inventory."""

from __future__ import annotations

import argparse
from collections import defaultdict
import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
ARCHITECTURE_PATH = ROOT / "workspace-architecture.json"
BINDINGS_PATH = ROOT / "crates/perry/well_known_bindings.toml"
CARGO_PATH = ROOT / "Cargo.toml"
DOC_PATH = ROOT / "docs/src/native-libraries/governance.md"
START = "<!-- BEGIN GENERATED BINDING GOVERNANCE -->"
END = "<!-- END GENERATED BINDING GOVERNANCE -->"

# Binding-specific migration targets refine workspace-architecture.json's
# general keep/externalize/remove decision without creating a second policy.
MIGRATIONS = {
    "core-runtime": {
        "decision": "keep",
        "category": "Runtime API",
        "target": "Keep near core; consolidate when practical",
        "status": "Bundled; retained",
    },
    "compile-source": {
        "decision": "externalize",
        "category": "Source package",
        "target": "Compile the upstream package source",
        "status": "Bundled; migration pending",
    },
    "external-package": {
        "decision": "externalize",
        "category": "External integration",
        "target": "Move to an external native package",
        "status": "Bundled; migration pending",
    },
    "remove": {
        "decision": "remove",
        "category": "Obsolete integration",
        "target": "Remove after compatibility review",
        "status": "Bundled; removal pending",
    },
}


def load_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def load_architecture() -> dict:
    with ARCHITECTURE_PATH.open(encoding="utf-8") as handle:
        return json.load(handle)


def binding_packages_by_crate() -> dict[str, list[str]]:
    packages: dict[str, list[str]] = defaultdict(list)
    for package, entry in load_toml(BINDINGS_PATH)["bindings"].items():
        packages[entry["crate"]].append(package)
    return {crate: sorted(names) for crate, names in packages.items()}


def workspace_extension_crates() -> set[str]:
    members = load_toml(CARGO_PATH)["workspace"]["members"]
    return {
        Path(member).name
        for member in members
        if Path(member).name.startswith("perry-ext-")
    }


def extension_directories() -> set[str]:
    return {path.name for path in (ROOT / "crates").glob("perry-ext-*") if path.is_dir()}


def governance_entries() -> dict[str, dict]:
    crates = load_architecture().get("crates")
    if not isinstance(crates, dict):
        raise ValueError(f"{ARCHITECTURE_PATH}: missing `crates` object")
    return {
        name: entry
        for name, entry in crates.items()
        if isinstance(entry, dict) and entry.get("category") == "binding"
    }


def format_set_difference(label: str, actual: set[str], expected: set[str]) -> list[str]:
    failures = []
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    if missing:
        failures.append(f"{label} is missing: {', '.join(missing)}")
    if extra:
        failures.append(f"{label} has unexpected entries: {', '.join(extra)}")
    return failures


def validate(entries: dict[str, dict]) -> tuple[list[str], dict[str, list[str]]]:
    failures: list[str] = []
    governed = set(entries)
    mapped = binding_packages_by_crate()
    mapped_crates = set(mapped)
    workspace = workspace_extension_crates()
    directories = extension_directories()

    failures += format_set_difference("architecture binding inventory", governed, directories)
    failures += format_set_difference("workspace extension members", workspace, directories)
    failures += format_set_difference("well-known binding crates", mapped_crates, directories)

    required_fields = {"category", "decision", "migration"}
    for crate, entry in sorted(entries.items()):
        fields = set(entry)
        missing_fields = required_fields - fields
        if missing_fields:
            failures.append(
                f"{crate}: missing required governance fields {sorted(missing_fields)}"
            )
            continue

        migration = entry["migration"]
        if not isinstance(migration, str) or migration not in MIGRATIONS:
            failures.append(f"{crate}: unknown migration target {migration!r}")
            continue
        expected_decision = MIGRATIONS[migration]["decision"]
        if entry["decision"] != expected_decision:
            failures.append(
                f"{crate}: migration {migration!r} requires decision "
                f"{expected_decision!r}, got {entry['decision']!r}"
            )

    return failures, mapped


def render_table(entries: dict[str, dict], packages_by_crate: dict[str, list[str]]) -> str:
    lines = [
        "| Crate | Package mapping(s) | Category | Migration target | Current status |",
        "|---|---|---|---|---|",
    ]
    for crate, entry in sorted(entries.items()):
        policy = MIGRATIONS[entry["migration"]]
        packages = "<br>".join(f"`{package}`" for package in packages_by_crate[crate])
        lines.append(
            f"| `{crate}` | {packages} | {policy['category']} | "
            f"{policy['target']} | {policy['status']} |"
        )
    return "\n".join(lines)


def check_doc(table: str) -> list[str]:
    if not DOC_PATH.is_file():
        return [f"missing governance document: {DOC_PATH}"]
    doc = DOC_PATH.read_text()
    expected = f"{START}\n{table}\n{END}"
    if expected not in doc:
        return [
            "docs/src/native-libraries/governance.md inventory is stale; "
            "replace the generated block with `python3 scripts/binding_governance.py --table`"
        ]
    return []


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="validate policy and docs")
    mode.add_argument("--table", action="store_true", help="print the generated Markdown table")
    args = parser.parse_args()

    try:
        entries = governance_entries()
        failures, packages_by_crate = validate(entries)
    except (
        json.JSONDecodeError,
        KeyError,
        OSError,
        TypeError,
        ValueError,
        tomllib.TOMLDecodeError,
    ) as error:
        print(f"binding governance error: {error}", file=sys.stderr)
        return 1

    if failures:
        for failure in failures:
            print(f"FAIL {failure}", file=sys.stderr)
        return 1

    table = render_table(entries, packages_by_crate)
    if args.table:
        print(table)
        return 0

    failures = check_doc(table)
    if failures:
        for failure in failures:
            print(f"FAIL {failure}", file=sys.stderr)
        return 1
    print(f"binding governance OK — {len(entries)} extension crates classified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
