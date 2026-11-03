#!/usr/bin/env python3
"""Compare JSON-emitting workloads with balanced forward/reverse run order."""
import argparse
import hashlib
import json
import math
import platform
import re
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", action="append", required=True, metavar="LABEL=PATH")
    parser.add_argument("--rounds", type=int, default=4)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("hardware counter collection requires macOS time -l")
    if args.rounds < 1:
        parser.error("rounds must be positive")
    binaries = {}
    for spec in args.binary:
        label, separator, path = spec.partition("=")
        if not separator or not label or label in binaries:
            parser.error("expected unique LABEL=PATH")
        binaries[label] = Path(path).resolve(strict=True)
    if len(binaries) < 2:
        parser.error("provide at least two labeled binaries (paths may be identical for a placebo)")
    arguments = args.arguments[1:] if args.arguments[:1] == ["--"] else args.arguments
    report = {
        "binaries": {
            label: {"path": str(path), "bytes": path.stat().st_size,
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            for label, path in binaries.items()
        },
        "arguments": arguments,
        "samples": [],
    }
    expected = None
    labels = list(binaries)
    for block in range(args.rounds):
        order = labels[block % len(labels):] + labels[:block % len(labels)]
        for position, label in enumerate(order + list(reversed(order))):
            started = time.time()
            processes = subprocess.check_output(["ps", "-A", "-o", "comm="], text=True)
            rustc_count = sum(Path(line.strip()).name == "rustc" for line in processes.splitlines())
            result = subprocess.run(
                ["/usr/bin/time", "-l", str(binaries[label]), *arguments],
                capture_output=True, text=True, check=True, timeout=300,
            )
            value = json.loads(result.stdout)
            milliseconds = value.get("milliseconds")
            if not isinstance(milliseconds, (int, float)) or not math.isfinite(milliseconds) or milliseconds <= 0:
                raise ValueError(f"invalid workload milliseconds: {milliseconds!r}")
            stable = {key: item for key, item in value.items() if key != "milliseconds"}
            if expected is None:
                expected = stable
            if stable != expected:
                raise ValueError(f"{label}: workload output changed: {stable!r} != {expected!r}")
            for key, suffix in (("instructions", "retired"), ("cycles", "elapsed")):
                match = re.search(rf"^\s*(\d+)\s+{key}(?: {suffix})?\s*$", result.stderr, re.MULTILINE)
                if not match or int(match[1]) == 0:
                    raise ValueError(f"missing {key} counter: {result.stderr}")
                value[key] = int(match[1])
            for key in ("real", "user", "sys"):
                match = re.search(rf"([\d.]+)\s+{key}\b", result.stderr)
                if not match:
                    raise ValueError(f"missing {key} time: {result.stderr}")
                value[key + "_seconds"] = float(match[1])
            value.update(label=label, block=block, position=position,
                         epoch=started, rustc_count=rustc_count)
            report["samples"].append(value)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(value), flush=True)


if __name__ == "__main__":
    main()
