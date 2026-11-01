#!/usr/bin/env python3
"""Serial, rotating-order measurements of the public #13662 scan fixture."""
import argparse
import hashlib
import json
import math
import platform
import re
import statistics
import subprocess
from pathlib import Path


def measure(binary, mode):
    # macOS time(1) counts the whole process, including startup and warmup.
    # The fixture's milliseconds field counts only the measured inner scan.
    result = subprocess.run(
        ["/usr/bin/time", "-l", str(binary), mode, "5000000"],
        capture_output=True, text=True, check=True,
    )
    value = json.loads(result.stdout)
    expected = dict(mode=mode, iterations=5000000, warmup=42188, checksum=2109378)
    for key, want in expected.items():
        if value.get(key) != want:
            raise ValueError(f"{binary}: {key}: expected {want}, got {value.get(key)}")
    milliseconds = value.get("milliseconds")
    if not isinstance(milliseconds, (int, float)) or not math.isfinite(milliseconds) or milliseconds <= 0:
        raise ValueError(f"{binary}: invalid elapsed milliseconds: {milliseconds!r}")
    for name, suffix in (("instructions", "retired"), ("cycles", "elapsed")):
        match = re.search(rf"^\s*(\d+)\s+{name}(?: {suffix})?\s*$", result.stderr, re.MULTILINE)
        if not match:
            raise ValueError(f"time -l did not report {name}: {result.stderr}")
        value[name] = int(match[1])
        if value[name] == 0:
            raise ValueError(f"time -l reported no {name}")
    real = re.search(r"([\d.]+)\s+real\b", result.stderr)
    if not real:
        raise ValueError(f"time -l did not report real time: {result.stderr}")
    value["process_seconds"] = float(real[1])
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", action="append", required=True, metavar="LABEL=PATH")
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("this counter collector requires macOS time -l; use perf stat on Linux")
    if args.rounds < 3:
        parser.error("at least three rounds are required")
    binaries = {}
    for spec in args.binary:
        label, separator, path = spec.partition("=")
        if not separator or not label or label in binaries:
            parser.error(f"expected unique LABEL=PATH, got {spec!r}")
        binaries[label] = Path(path).resolve(strict=True)
    report = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "binaries": {
            label: {"path": str(path), "bytes": path.stat().st_size,
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            for label, path in binaries.items()
        },
        "samples": [],
    }
    labels = list(binaries)
    for round_index in range(args.rounds):
        order = labels[round_index % len(labels):] + labels[:round_index % len(labels)]
        for mode in ("forof", "indexed"):
            for label in order:
                sample = measure(binaries[label], mode)
                sample.update(label=label, round=round_index)
                report["samples"].append(sample)
                print(json.dumps(sample), flush=True)
    report["medians"] = {}
    for label in labels:
        report["medians"][label] = {}
        for mode in ("forof", "indexed"):
            samples = [s for s in report["samples"] if s["label"] == label and s["mode"] == mode]
            report["medians"][label][mode] = {
                key: statistics.median(s[key] for s in samples)
                for key in ("milliseconds", "instructions", "cycles", "process_seconds")
            }
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["medians"], indent=2))


if __name__ == "__main__":
    main()
