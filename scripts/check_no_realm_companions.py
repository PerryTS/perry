#!/usr/bin/env python3
"""Ratchet on checked-in realm snapshots in crate source trees.

Only the companions listed in ALLOWED may exist, each with at most the listed
number of declarations. A listed file may shrink or disappear; no other
companion may appear and no listed one may grow.
"""
import argparse
import json
from pathlib import Path

ALLOWED = {
    "crates/perry-runtime/src/closure/registry.realm.json": 8,
    "crates/perry-runtime/src/navigator.realm.json": 1,
    "crates/perry-runtime/src/object/global_this/array_error.realm.json": 1,
    "crates/perry-runtime/src/object/global_this/populate.realm.json": 26,
    "crates/perry-runtime/src/object/native_module.realm.json": 6,
    "crates/perry-runtime/src/object/native_module/constructor_shapes.realm.json": 2,
}


def companions(root):
    crates = root / "crates"
    if not crates.is_dir():
        raise ValueError(f"missing crates directory: {crates}")
    return sorted(path.relative_to(root) for path in crates.glob("*/src/**/*.realm.json"))


def declarations(path):
    groups = json.loads(path.read_text())["groups"]
    return sum(len(entries) for entries in groups.values())


def violations(root, allowed=ALLOWED):
    found = []
    for relative in companions(root):
        key = relative.as_posix()
        if key not in allowed:
            found.append(f"{key}: not an allowed companion")
            continue
        count = declarations(root / relative)
        if count > allowed[key]:
            found.append(f"{key}: {count} declarations, allowed at most {allowed[key]}")
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        found = violations(args.root)
    except ValueError as error:
        parser.exit(1, str(error) + "\n")
    if found:
        parser.exit(1, "realm companion ratchet failed:\n" + "\n".join(found) + "\n")
    print("realm companions are within the ratchet")


if __name__ == "__main__":
    main()
