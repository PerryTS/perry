#!/usr/bin/env python3
"""Count all try-frame pushes with a temporary uprobe, outside perf A/B runs."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    symbols = subprocess.check_output(["nm", "--defined-only", str(args.binary)], text=True)
    push = [line.split()[-1] for line in symbols.splitlines() if "try_push_with_kind" in line]
    assert len(push) == 1, push
    event = f"perry_catch_{os.getpid()}:push"
    # Use the DWARF function name: an LTO-generated `.llvm.<hash>` suffix in
    # the linkage name is parsed as probe syntax by perf.
    subprocess.run(["perf", "probe", "-x", str(args.binary), "--add", f"{event}=try_push_with_kind"], stdout=subprocess.DEVNULL, check=True)
    try:
        with args.output.with_suffix(".out").open("w") as stream:
            subprocess.run(["perf", "stat", "-x", ";", "-e", event, "-o", str(args.output), "--", str(args.binary)], stdout=stream, check=True)
    finally:
        subprocess.run(["perf", "probe", "-q", "--del", event], check=True)
    count = next(int(line.split(";")[0]) for line in args.output.read_text().splitlines() if event in line)
    print(json.dumps({"binary": str(args.binary), "frame_pushes": count}))


if __name__ == "__main__":
    main()
