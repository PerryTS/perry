#!/usr/bin/env python3
"""Reject checked-in realm snapshots in crate source trees."""
import argparse
from pathlib import Path


def companions(root):
    crates = root / "crates"
    if not crates.is_dir():
        raise ValueError(f"missing crates directory: {crates}")
    return sorted(path.relative_to(root) for path in crates.glob("*/src/**/*.realm.json"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        found = companions(args.root)
    except ValueError as error:
        parser.exit(1, str(error) + "\n")
    if found:
        parser.exit(1, f"{len(found)} realm companions remain under crates/*/src:\n" +
                    "\n".join(str(path) for path in found[:10]) + "\n")
    print("realm source contains no JSON companions")


if __name__ == "__main__":
    main()
