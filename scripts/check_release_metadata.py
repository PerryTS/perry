#!/usr/bin/env python3
"""Validate the release metadata contract without interpreting prose docs."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = REPO_ROOT / "Cargo.toml"
CHANGELOG = REPO_ROOT / "CHANGELOG.md"
RELEASE_WORKFLOW = REPO_ROOT / ".github/workflows/release-packages.yml"


def workspace_version() -> str:
    """Read version from the structured [workspace.package] TOML section."""
    in_workspace_package = False
    for line in CARGO_TOML.read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            in_workspace_package = line == "[workspace.package]"
            continue
        if in_workspace_package:
            match = re.fullmatch(r'\s*version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*(?:#.*)?', line)
            if match:
                return match.group(1)
    raise ValueError("Cargo.toml has no [workspace.package].version")


def first_release_heading() -> str:
    for line in CHANGELOG.read_text(encoding="utf-8").splitlines():
        match = re.match(r"^## v([0-9]+\.[0-9]+\.[0-9]+)(?:\s|$)", line)
        if match:
            return match.group(1)
    raise ValueError("CHANGELOG.md has no release heading of the form '## vX.Y.Z'")


def trigger_block(workflow: str) -> list[str]:
    """Return the top-level YAML `on` block, using indentation rather than prose."""
    lines = workflow.splitlines()
    for index, line in enumerate(lines):
        if line == "on:":
            block: list[str] = []
            for candidate in lines[index + 1 :]:
                if candidate and not candidate.startswith((" ", "\t", "#")):
                    break
                block.append(candidate)
            return block
    raise ValueError("release workflow has no top-level 'on' block")


def has_published_release_trigger(block: list[str]) -> bool:
    for index, line in enumerate(block):
        if line == "  release:":
            child_lines: list[str] = []
            for candidate in block[index + 1 :]:
                if candidate.startswith("  ") and not candidate.startswith("    "):
                    break
                child_lines.append(candidate)
            return any(re.fullmatch(r"\s*types:\s*\[\s*published\s*\]\s*", line) for line in child_lines)
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--print-version", action="store_true", help="print the validated workspace version")
    args = parser.parse_args()

    try:
        version = workspace_version()
        changelog_version = first_release_heading()
        triggers = trigger_block(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    except ValueError as error:
        print(f"release metadata check failed: {error}", file=sys.stderr)
        return 1

    errors: list[str] = []
    if changelog_version != version:
        errors.append(
            f"CHANGELOG.md starts with v{changelog_version}, but Cargo.toml declares v{version}"
        )
    if not has_published_release_trigger(triggers):
        errors.append("release-packages.yml must trigger on release publication (types: [published])")
    if "  workflow_dispatch:" not in triggers:
        errors.append("release-packages.yml must retain workflow_dispatch for manual recovery")

    if errors:
        for error in errors:
            print(f"release metadata check failed: {error}", file=sys.stderr)
        return 1

    if args.print_version:
        print(version)
    else:
        print(
            f"Release metadata is valid: Cargo.toml and CHANGELOG.md agree on v{version}; "
            "release-packages.yml publishes on GitHub Release publication."
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
