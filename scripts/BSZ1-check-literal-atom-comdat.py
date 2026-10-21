#!/usr/bin/env python3
"""Compare ELF section bytes for two builds of test_gap_bsz1_literal_atom_comdat.

Compile the fixture with the base/head compilers and the same runtime/flags,
check each output against Node, then run this script with the two executables.
The fixture's eight modules repeat literals and shapes; both sections must shrink.
"""
import argparse
from pathlib import Path
import subprocess


def sizes(path):
    result = {"file": Path(path).stat().st_size}
    output = subprocess.check_output(["size", "-A", path], text=True)
    for line in output.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[1].isdigit():
            result[parts[0]] = int(parts[1])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("head")
    args = parser.parse_args()
    base, head = sizes(args.base), sizes(args.head)
    print("section\tbase_bytes\thead_bytes\tdelta_bytes")
    for section in ["file", ".text", "perry_strings", "perry_atoms"]:
        before, after = base[section], head[section]
        print(f"{section}\t{before}\t{after}\t{after-before}")
    for section in ["perry_strings", "perry_atoms"]:
        if head[section] >= base[section]:
            raise SystemExit(f"{section}: repeated module data did not fold")


if __name__ == "__main__":
    main()
