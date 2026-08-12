#!/usr/bin/env python3
"""Reject GC environment knobs that no live parser owns.

Current documentation, executable scripts, workflow configuration, and the
generated gettext catalogs are claims about supported controls.  This checker
extracts those claims and compares them with direct environment reads in the
runtime, code generator, and compiler.  Historical experiment journals are
named exemptions: they preserve evidence without licensing a dead knob in a
live gate or current reference page.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Keep the never-shipped nursery spelling visible to the scanner without making
# this regex definition itself look like a current documentation claim.
NEVER_SHIPPED_NURSERY = r"PERRY_" r"NURSERY_MB"
KNOB_RE = re.compile(
    rf"\b(?:PERRY_GEN_GC(?:_[A-Z0-9_]+)?|PERRY_GC_[A-Z0-9_]+|"
    rf"{NEVER_SHIPPED_NURSERY}|PERRY_WRITE_BARRIERS|PERRY_SHADOW_STACK|"
    rf"PERRY_RS4GC|PERRY_STACKMAP_WALKER|PERRY_CONSERVATIVE_STACK_SCAN)\b"
)
PARSER_RE = re.compile(
    r'(?:std::env::var(?:_os)?|env_var)\(\s*"(PERRY_[A-Z0-9_]+)"\s*\)'
)

PARSER_ROOTS = (
    "crates/perry-runtime/src",
    "crates/perry-codegen/src",
    "crates/perry/src",
)
CLAIM_ROOTS = (
    "docs",
    "scripts",
    "benchmarks/gc_ratchet",
    ".github/workflows",
)
CLAIM_SUFFIXES = {".json", ".md", ".po", ".pot", ".py", ".sh", ".yaml", ".yml"}

# These are chronological evidence, not current reference documentation.  Keep
# this list path-exact: adding a whole directory would let a new current page
# inherit an exemption accidentally.
HISTORICAL_DOCS = {
    "docs/ecs-perf-case-study.md",
    "docs/generational-gc-plan.md",
    "docs/statepoint-gc-experiment.md",
}

# Script-owned output plumbing shares the PERRY_GC_ prefix but is intentionally
# not parsed by runtime/codegen.  Each exception names the executable owner and
# is checked for staleness too.
SCRIPT_OWNED = {
    "PERRY_GC_EVIDENCE_DIR": "scripts/run_memory_stability_tests.sh",
}


def strip_rust_comments(source: str) -> str:
    """Remove comments so a deleted parser cannot survive as coverage."""
    without_blocks = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    return re.sub(r"//[^\n]*", "", without_blocks)


def production_rust_files(root: Path):
    for base in PARSER_ROOTS:
        for path in (root / base).rglob("*.rs"):
            rel = path.relative_to(root)
            if "tests" in rel.parts or path.name == "tests.rs" or path.name.endswith("_tests.rs"):
                continue
            yield path


def parsed_knobs(root: Path) -> dict[str, set[str]]:
    found: dict[str, set[str]] = defaultdict(set)
    for path in production_rust_files(root):
        rel = path.relative_to(root).as_posix()
        source = strip_rust_comments(path.read_text(encoding="utf-8"))
        for name in PARSER_RE.findall(source):
            found[name].add(rel)
    return found


def claim_files(root: Path):
    yield root / "CLAUDE.md"
    for base in CLAIM_ROOTS:
        for path in (root / base).rglob("*"):
            if path.is_file() and path.suffix in CLAIM_SUFFIXES:
                rel = path.relative_to(root).as_posix()
                if rel not in HISTORICAL_DOCS:
                    yield path


def claimed_knobs(root: Path) -> dict[str, set[str]]:
    found: dict[str, set[str]] = defaultdict(set)
    for path in claim_files(root):
        rel = path.relative_to(root).as_posix()
        for name in KNOB_RE.findall(path.read_text(encoding="utf-8")):
            found[name].add(rel)
    return found


def problems_for(
    claims: dict[str, set[str]], parsers: dict[str, set[str]], root: Path
) -> list[str]:
    problems = []
    allowed = set(parsers) | set(SCRIPT_OWNED)
    for name in sorted(set(claims) - allowed):
        paths = ", ".join(sorted(claims[name]))
        problems.append(f"{name}: claimed by {paths}, but no live parser owns it")
    for name, owner in SCRIPT_OWNED.items():
        owner_path = root / owner
        if not owner_path.is_file() or name not in owner_path.read_text(encoding="utf-8"):
            problems.append(f"{name}: script-owned exception is stale; expected it in {owner}")
    return problems


def self_test() -> int:
    live = "PERRY_GC_" + "LIVE"
    deleted = "PERRY_GEN_GC_" + "DELETED"
    source = (
        f'let _ = std::env::var("{live}");\n'
        f'// let _ = std::env::var("{deleted}");\n'
    )
    extracted = set(PARSER_RE.findall(strip_rust_comments(source)))
    failures = []
    if extracted != {live}:
        failures.append(f"comment stripping admitted the wrong parser set: {sorted(extracted)}")

    claims = {live: {"docs/current.md"}, deleted: {"scripts/live.sh"}}
    found = problems_for(claims, {live: {"runtime.rs"}}, REPO)
    if not any(deleted in problem for problem in found):
        failures.append("a claimed knob with no parser passed")
    if any(live in problem for problem in found):
        failures.append("a claimed knob with a live parser failed")

    historical = Path("docs/generational-gc-plan.md").as_posix()
    if historical not in HISTORICAL_DOCS:
        failures.append("the historical generational plan lost its exact exemption")

    for failure in failures:
        print(f"GC env-knob self-test FAILED: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("GC env-knob self-test: OK (dead and commented parsers are rejected)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    parsers = parsed_knobs(REPO)
    claims = claimed_knobs(REPO)
    problems = problems_for(claims, parsers, REPO)
    if problems:
        print("GC environment-knob drift check FAILED:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            "Delete or replace the stale claim, add a real production parser, "
            "or name a genuinely historical document in HISTORICAL_DOCS.",
            file=sys.stderr,
        )
        return 1
    print(
        f"GC environment-knob drift check OK: {len(claims)} claimed knobs, "
        f"{len(parsers)} live env parsers, {len(HISTORICAL_DOCS)} historical documents exempt"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
