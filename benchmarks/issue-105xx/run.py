#!/usr/bin/env python3
"""Run the four issue reproductions with matching compiler/runtime artifacts."""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile

CASES = {
    10519: (["split", "split_any", "split_limit1", "split_short", "indexof", "indexof_limit1"], 100000),
    10525: (["direct", "queueMicrotask", "promise", "nextTick", "setImmediate"], 1000),
    10526: (["cork", "concat", "direct"], 2000),
    10528: (["interp", "static"], 20000),
}


def run(command, env):
    result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError(f"{command}: exit {result.returncode}\n{result.stderr[-4000:]}")
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()), PERRY_NO_AUTO_OPTIMIZE="1")
    records = []
    with tempfile.TemporaryDirectory(prefix="perry-105xx-bench-") as temp:
        for issue, (variants, count) in CASES.items():
            source = Path(__file__).parent / f"issue_{issue}.ts"
            binary = Path(temp) / str(issue)
            run([str(args.perry.resolve()), "compile", str(source.resolve()), "--no-auto-optimize", "--no-cache", "-o", str(binary)], env)
            for variant in variants:
                row = {"issue": issue, "variant": variant, "iterations": count}
                expected = None
                for engine, command in [("node", ["node", str(source)]), ("perry", [str(binary)])]:
                    timings = []
                    for _ in range(args.repeats):
                        output = run(command + [variant, str(count)], env)
                        checksum = re.search(r"checksum=(\d+)", output).group(1)
                        sample = re.search(r"sample=(.*)", output)
                        answer = (checksum, sample.group(1) if sample else None)
                        if expected is None:
                            expected = answer
                        if answer != expected:
                            raise AssertionError(f"{issue} {variant}: {engine} {answer} != {expected}")
                        timings.append(float(re.search(r"ms=([\d.]+)", output).group(1)))
                    row[f"{engine}_ms"] = statistics.median(timings)
                    row[f"{engine}_runs_ms"] = timings
                row["checksum"] = expected[0]
                records.append(row)
                print(json.dumps(row), flush=True)
    args.output.write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
