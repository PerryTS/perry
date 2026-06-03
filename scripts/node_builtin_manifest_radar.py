#!/usr/bin/env python3
"""Check Node builtin modules against Perry's manifest and parity skiplist.

The radar answers one narrow question: every builtin module reported by
Node must either be claimed by Perry's API manifest or explicitly skiplisted
with a reason. The parity suite directory map is included in the report so
suite-only drift is visible while triaging manifest gaps.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib  # type: ignore


REPO_ROOT = Path(__file__).resolve().parent.parent
SKIPLIST = REPO_ROOT / "scripts" / "parity-skiplist.toml"
SUITE_DIR = REPO_ROOT / "test-parity" / "node-suite"

SUITE_ALIASES = {
    "fs-promises": "fs/promises",
    "inspector-promises": "inspector/promises",
}


def normalize_module(name: str) -> str:
    if name.startswith("node:"):
        return name.removeprefix("node:")
    return name


def load_skip_modules(path: Path) -> set[str]:
    with path.open("rb") as f:
        data = tomllib.load(f)
    return {normalize_module(name) for name in data.get("skip-modules", {})}


def load_manifest_modules(manifest_json: str) -> set[str]:
    manifest = json.loads(manifest_json)
    entries = manifest.get("entries", [])
    return {normalize_module(entry["module"]) for entry in entries if "module" in entry}


def load_manifest_from_perry(perry: str) -> str:
    proc = subprocess.run(
        [perry, "--print-api-manifest=json"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return proc.stdout


def load_node_builtins(node: str) -> set[str]:
    script = (
        "console.log(JSON.stringify("
        "require('module').builtinModules.map((m) => m.replace(/^node:/, '')).sort()"
        "))"
    )
    proc = subprocess.run(
        [node, "-e", script],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return {normalize_module(name) for name in json.loads(proc.stdout)}


def load_suite_modules(path: Path) -> set[str]:
    if not path.exists():
        return set()

    modules: set[str] = set()
    for child in path.iterdir():
        if not child.is_dir():
            continue
        name = SUITE_ALIASES.get(child.name, child.name)
        modules.add(name)
        for subdir in child.iterdir():
            if not subdir.is_dir():
                continue
            candidate = f"{child.name}/{subdir.name}"
            modules.add(SUITE_ALIASES.get(candidate, candidate))
    return modules


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--node", default="node", help="Node executable to query")
    parser.add_argument(
        "--perry",
        default=str(REPO_ROOT / "target" / "release" / "perry"),
        help="Perry binary used to emit --print-api-manifest=json",
    )
    parser.add_argument(
        "--manifest-json",
        type=Path,
        help="Read an existing Perry manifest JSON file instead of running --perry",
    )
    parser.add_argument("--skiplist", type=Path, default=SKIPLIST)
    parser.add_argument("--suite-dir", type=Path, default=SUITE_DIR)
    args = parser.parse_args()

    node_builtins = load_node_builtins(args.node)
    if args.manifest_json:
        manifest_json = args.manifest_json.read_text()
    else:
        manifest_json = load_manifest_from_perry(args.perry)
    manifest_modules = load_manifest_modules(manifest_json)
    skip_modules = load_skip_modules(args.skiplist)
    suite_modules = load_suite_modules(args.suite_dir)

    unclassified = sorted(node_builtins - manifest_modules - skip_modules)
    suite_only = sorted((node_builtins & suite_modules) - manifest_modules - skip_modules)

    if unclassified:
        print("Unclassified Node builtin modules:", file=sys.stderr)
        for module in unclassified:
            suite_note = " suite=yes" if module in suite_modules else " suite=no"
            print(f"  - {module}{suite_note}", file=sys.stderr)
        print(
            "\nAdd a Perry API manifest entry or a scripts/parity-skiplist.toml "
            "skip-modules reason for each module above.",
            file=sys.stderr,
        )
        return 1

    print(
        "Node builtin manifest radar clean: "
        f"{len(node_builtins)} Node builtins, "
        f"{len(manifest_modules)} manifest modules, "
        f"{len(skip_modules)} skiplisted modules."
    )
    if suite_only:
        print(
            "Suite-only builtins with no manifest/skiplist classification: "
            + ", ".join(suite_only)
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
