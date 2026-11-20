#!/usr/bin/env python3
"""Measure #13662's scan with serial, rotated arms and checked results.

Pass already stripped executables, built with the same compiler/app flags.
macOS uses time -l; Linux uses perf stat (hardware-counter access required).
The raw JSON keeps every observation, including whole-process counters and
wall time separately from the fixture's post-warmup scan time.
"""

import argparse
import json
import math
import platform
import re
import statistics
import subprocess
import time
from pathlib import Path


def checksum(iterations):
    steps = [-17, -16, -15, -1, 1, 15, 16, 17]
    period = [
        int(any(0 <= i + d < 128 and ((i + d) & 0x88) == 0
                and ((i + d) * 3) % 7 - 3 == 2 for d in steps))
        for i in range(128)
    ]
    return iterations // 128 * sum(period) + sum(period[:iterations % 128])


def run(binary, mode, iterations):
    args = [str(binary), mode, str(iterations)]
    if platform.system() == "Darwin":
        command = ["/usr/bin/time", "-l", *args]
    elif platform.system() == "Linux":
        command = ["perf", "stat", "-x", ";", "--no-big-num", "-e", "instructions,cycles", "--", *args]
    else:
        raise SystemExit("Hardware-counter collection supports macOS and Linux only")
    started = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    elapsed = (time.perf_counter() - started) * 1000
    if result.returncode:
        raise RuntimeError(f"{command}: exit {result.returncode}\n{result.stderr}")
    value = json.loads(result.stdout)
    expected = dict(mode=mode, iterations=iterations, warmup=checksum(100000),
                    checksum=checksum(iterations))
    for key, want in expected.items():
        if value.get(key) != want:
            raise RuntimeError(f"{binary}: {key}: {value.get(key)!r} != {want!r}")
    if not math.isfinite(value["milliseconds"]) or value["milliseconds"] < 0:
        raise RuntimeError(f"{binary}: invalid scan time: {value['milliseconds']!r}")
    counts = {}
    for event in ("instructions", "cycles"):
        if platform.system() == "Darwin":
            suffix = "retired" if event == "instructions" else "elapsed"
            match = re.search(rf"^\s*(\d+)\s+{event} {suffix}\s*$", result.stderr, re.M)
        else:
            match = re.search(rf"^([0-9]+);[^;]*;{event};", result.stderr, re.M)
        if not match:
            raise RuntimeError(f"Missing {event} counter (not zero):\n{result.stderr}")
        counts[event] = int(match[1])
    return dict(scan_ms=value["milliseconds"], process_ms=elapsed, **counts,
                stdout=result.stdout, counters=result.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arm", action="append", required=True, metavar="NAME=EXECUTABLE")
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--iterations", type=int, default=5000000)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.rounds < 1 or args.iterations < 1:
        parser.error("rounds and iterations must be positive")
    arms = {}
    for arm in args.arm:
        name, path = arm.split("=", 1)
        if name in arms:
            parser.error(f"duplicate arm: {name}")
        arms[name] = Path(path).resolve(strict=True)
    report = dict(platform=platform.platform(), machine=platform.machine(),
                  iterations=args.iterations,
                  bytes={name: path.stat().st_size for name, path in arms.items()},
                  samples=[], medians={})
    names = list(arms)
    for round_index in range(args.rounds):
        order = names[round_index % len(names):] + names[:round_index % len(names)]
        for mode in ("forof", "indexed"):
            for name in order:
                sample = dict(arm=name, mode=mode, round=round_index,
                              **run(arms[name], mode, args.iterations))
                report["samples"].append(sample)
                print(f"{name} {mode} {round_index}: {sample['scan_ms']:.3f} ms", flush=True)
    for name in names:
        report["medians"][name] = {}
        for mode in ("forof", "indexed"):
            samples = [s for s in report["samples"] if s["arm"] == name and s["mode"] == mode]
            report["medians"][name][mode] = {
                key: statistics.median(s[key] for s in samples)
                for key in ("scan_ms", "process_ms", "instructions", "cycles")
            }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["medians"], indent=2))


if __name__ == "__main__":
    main()
