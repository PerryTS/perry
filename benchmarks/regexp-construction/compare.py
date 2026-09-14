#!/usr/bin/env python3
"""Interleaved before/after runtime benchmark checks, with the pinned Node oracle.

Each directory must hold a compiler and the static archives from the same build.
Run this on the build host. Timing is child user CPU, avoiding scheduler wait.
"""
import argparse
import json
import os
from pathlib import Path
import resource
import shutil
import statistics
import subprocess
import time


def positive_integer(value):
    try:
        number = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("expected a positive integer") from None
    if number < 1:
        raise argparse.ArgumentTypeError("expected a positive integer")
    return number


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--before", type=Path, required=True)
parser.add_argument("--after", type=Path)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--runs", type=positive_integer, default=12)
parser.add_argument("--filter", default="")
parser.add_argument("--paired-controls", action="store_true", help="Add two identical-copy labels per build to each round")
args = parser.parse_args()
if args.paired_controls and args.after is None:
    parser.error('--paired-controls requires --after')
args.output.mkdir(parents=True, exist_ok=True)
runner = args.output / "run-benchmark"
root = Path(__file__).resolve().parents[2]
rows = []


def run(command, **kwargs):
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_utime
    start = time.monotonic()
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=180, **kwargs)
    cpu = resource.getrusage(resource.RUSAGE_CHILDREN).ru_utime - before
    return result, cpu, time.monotonic() - start


for source in sorted((root / "benchmarks/app-patterns/kernels").glob("*.ts")):
    if args.filter not in source.stem:
        continue
    row = {"name": source.stem}
    binaries = {}
    for label, directory in [("before", args.before), ("after", args.after)]:
        if directory is None:
            continue
        binary = args.output / f"{source.stem}-{label}"
        environment = dict(os.environ, PERRY_RUNTIME_DIR=str(directory))
        result, _, _ = run([str(directory / "perry"), "compile", str(source),
                            "--no-auto-optimize", "-o", str(binary)], env=environment)
        (args.output / f"{source.stem}-{label}.compile.log").write_bytes(result.stderr)
        if result.returncode:
            row[label + "_error"] = "compile"
        else:
            binaries[label] = binary
    oracle, _, _ = run(["node", "--experimental-strip-types", str(source)])
    expected = oracle.stdout.strip()
    row["node_exit"] = oracle.returncode
    samples = {label: [] for label in binaries}
    if args.paired_controls:
        for label in ('before', 'after'):
            binaries[label + '_control'] = binaries[label]
            samples[label + '_control'] = []
    for iteration in range(args.runs + 1):
        order = list(binaries)
        if args.paired_controls:
            order = [('before', 'after', 'after_control', 'before_control'),
                     ('after', 'before', 'before_control', 'after_control'),
                     ('before_control', 'after_control', 'after', 'before'),
                     ('after_control', 'before_control', 'before', 'after')][iteration % 4]
        elif iteration % 2:
            order.reverse()
        for label in order:
            # argv[0]/execPath length can change startup allocation and GC
            # layout. An A/A control showed ~1.7% for byte-identical binaries
            # named *-before and *-after. Copy outside the timed window and
            # execute both builds at the same pathname/inode.
            shutil.copyfile(binaries[label], runner)
            runner.chmod(0o755)
            result, cpu, wall = run([str(runner)])
            if result.returncode or result.stdout.strip() != expected:
                row[label + "_error"] = {
                    "exit": result.returncode,
                    "stdout": result.stdout.decode(errors="replace")[:500],
                    "node": expected.decode(errors="replace")[:500],
                }
            if iteration:
                samples[label].append({"cpu_ms": cpu * 1000, "wall_ms": wall * 1000})
    for label, values in samples.items():
        row[label] = values
        row[label + "_median_cpu_ms"] = statistics.median(v["cpu_ms"] for v in values)
    rows.append(row)
    (args.output / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
    print(json.dumps({k: v for k, v in row.items() if k not in samples}), flush=True)

if any(row["node_exit"] != 0 or any(key.endswith("_error") for key in row) for row in rows):
    raise SystemExit("A compiler, runtime, or Node oracle failed; see results.json")
