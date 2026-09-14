#!/usr/bin/env python3
"""Build/check probes, count user instructions, and retain folded DWARF stacks."""
import argparse
import collections
import json
import os
from pathlib import Path
import re
import subprocess


def run(args, output, env=None):
    with output.open("w") as stream:
        subprocess.run(args, stdout=stream, stderr=subprocess.STDOUT, env=env, check=True)


def fold(text):
    stacks = collections.Counter()
    frames = []
    for line in text.splitlines() + [""]:
        if not line.strip():
            if frames:
                stacks[";".join(reversed(frames))] += 1
            frames = []
        elif line[0].isspace():
            parts = line.strip().split(None, 1)
            if len(parts) == 2:
                symbol = parts[1].rsplit(" (", 1)[0]
                frames.append(re.sub(r"\+0x[0-9a-f]+$", "", symbol))
    return stacks


def summarize(instructions, stacks):
    # LTO can inline capture into either exported entry point. Counting only
    # the private helper would miss those frames. Never count a callback body
    # just because catch_js_throw / perry_sjlj_try appears above it.
    capture_pattern = re.compile(r"\b(?:try_push_with_kind|js_try_push|js_eh_try_push)\b|CatchSavepoint::capture")
    capture = sum(n for s, n in stacks.items() if capture_pattern.search(s))
    transport = sum(n for s, n in stacks.items() if re.search(r"perry_sjlj_try|arm_trap_and_run|sigsetjmp|__sigjmp_save|js_try_end|catch_js_throw", s.split(";")[-1]))
    total = sum(stacks.values())
    return {"instructions": instructions, "samples": total, "capture_samples": capture, "transport_leaf_samples": transport, "capture_percent": 100 * capture / total}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--micro", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--period", type=int, default=1000003)
    parser.add_argument("--probes", nargs="*", default=["promises", "json", "hoist", "exec1"])
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_KEEP_SYMBOLS="1", PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_TELEMETRY="1")
    results = {}
    jobs = []
    for name in args.probes:
        source = Path(__file__).resolve().parent / (name + ".ts")
        binary = args.output / name
        run([str(args.perry), "compile", str(source), "--no-auto-optimize", "-o", str(binary)], args.output / (name + ".compile.log"), env)
        expected = subprocess.check_output(["node", str(source)], env=env)
        actual = subprocess.check_output([str(binary)], env=env)
        (args.output / (name + ".expected")).write_bytes(expected)
        (args.output / (name + ".out")).write_bytes(actual)
        assert actual == expected, name
        jobs.append((name, [str(binary)]))
    if args.micro:
        for mode in ["plain", "catch", "unwind"]:
            jobs.append((mode, [str(args.micro), mode, "1000000"]))
    for name, command in jobs:
        stat = args.output / (name + ".stat")
        run(["perf", "stat", "-x", ";", "-r", "3", "-e", "instructions:u", "-o", str(stat), "--"] + command, args.output / (name + ".stat.out"), env)
        instructions = next(float(line.split(";")[0]) for line in stat.read_text().splitlines() if "instructions:u" in line)
        data = args.output / (name + ".perf.data")
        run(["perf", "record", "-q", "-m", "64M", "-e", "instructions:u", "-c", str(args.period), "--call-graph", "dwarf", "-o", str(data), "--"] + command, args.output / (name + ".profile.out"), env)
        stacks = fold(subprocess.check_output(["perf", "script", "--no-inline", "-i", str(data)], text=True))
        (args.output / (name + ".folded")).write_text("".join(f"{s} {n}\n" for s, n in stacks.most_common()))
        results[name] = summarize(instructions, stacks)
        print(name, results[name], flush=True)
        (args.output / "results.json").write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
