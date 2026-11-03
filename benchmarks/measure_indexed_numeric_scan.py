#!/usr/bin/env python3
"""Instruction slopes for #13661 on macOS; checks every result against Node.

Compile bench_indexed_numeric_scan.ts with both compilers, using the same
runtime archives and options, then pass the two executables to this script.
"""
import argparse
import csv
import json
import pathlib
import re
import statistics
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("baseline", type=pathlib.Path)
parser.add_argument("candidate", type=pathlib.Path)
parser.add_argument("--node", default="node")
parser.add_argument("--rounds", type=int, default=3)
parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("indexed-scan.csv"))
args = parser.parse_args()
if sys.platform != "darwin" or args.rounds < 1:
    parser.error("requires macOS instruction counters and at least one round")
source = pathlib.Path(__file__).with_name("bench_indexed_numeric_scan.ts")
variants = ["method", "local", "live-bound", "fixed-bound", "fixed-typed",
            "directions", "arithmetic", "field", "typed-local", "flat"]
arms = {"baseline": [str(args.baseline.resolve())],
        "candidate": [str(args.candidate.resolve())],
        "node": [args.node, str(source)]}
rows = []
with args.output.open("w", newline="") as output:
    writer = csv.DictWriter(output, lineterminator="\n", fieldnames=["arm", "variant", "round", "N",
                                               "milliseconds", "instructions", "checksum"])
    writer.writeheader()
    for round_index in range(args.rounds):
        rotation = round_index % len(variants)
        order = variants[rotation:] + variants[:rotation]
        arm_order = list(arms)
        rotation = round_index % len(arm_order)
        arm_order = arm_order[rotation:] + arm_order[:rotation]
        for variant in order:
            for arm in arm_order:
                sizes = [5000000, 10000000] if round_index % 2 == 0 else [10000000, 5000000]
                for size in sizes:
                    result = subprocess.run(["/usr/bin/time", "-l", *arms[arm], variant, str(size)],
                                            text=True, capture_output=True, check=True, timeout=120)
                    data = json.loads(result.stdout)
                    expected = {5000000: 2109378, 10000000: 4218750}[size]
                    if data["warmup"] != 42188 or data["checksum"] != expected:
                        raise RuntimeError(f"{arm}/{variant}/{size}: wrong result {data}")
                    counter = re.search(r"(\d+)\s+instructions retired", result.stderr)
                    if counter is None:
                        raise RuntimeError(f"instruction counter missing: {result.stderr}")
                    row = dict(arm=arm, variant=variant, round=round_index, N=size,
                               milliseconds=data["milliseconds"], instructions=int(counter[1]),
                               checksum=data["checksum"])
                    rows.append(row)
                    writer.writerow(row)
                    output.flush()
            print(f"round {round_index + 1}: {variant}", flush=True)
summary = {}
for variant in variants:
    summary[variant] = {}
    for arm in arms:
        samples = [r for r in rows if r["variant"] == variant and r["arm"] == arm]
        def median(size, key):
            return statistics.median(r[key] for r in samples if r["N"] == size)
        summary[variant][arm] = {
            "milliseconds_5m": median(5000000, "milliseconds"),
            "instructions_per_probe": (median(10000000, "instructions") -
                                       median(5000000, "instructions")) / 5000000,
        }
args.output.with_suffix(".json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
