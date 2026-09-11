#!/usr/bin/env python3
"""Sequential, checksum-gated #10054 sweep; build matching release artifacts first."""

import argparse
import hashlib
import json
import math
import os
import platform
import subprocess
from pathlib import Path


def slope(rows):
    x = [math.log(row["n"]) for row in rows]
    y = [math.log(row["ms_per_run"]) for row in rows]
    mx, my = sum(x) / len(x), sum(y) / len(y)
    return sum((a - mx) * (b - my) for a, b in zip(x, y)) / sum((a - mx) ** 2 for a in x)


def sources(directory):
    ascii_source = (directory / "string-trim-ascii.ts").read_text()
    unicode_source = (directory / "string-trim-unicode.ts").read_text()
    alternating = ascii_source.replace(
        "function setup(n: number): string { return ' \\t' + \"aBcD\".repeat(n) + '\\n '; }",
        "function setup(n: number): string[] { return [' \\t' + \"aBcD\".repeat(n) + '\\n ', ' \\t' + \"aBcD\".repeat(n) + 'x\\n ']; }",
    ).replace("function run(input: string)", "function run(input: string[])")
    return {
        "string-trim-ascii": ascii_source,
        "string-trim-unicode": unicode_source,
        "unicode-trim-only": unicode_source.replace("hashString(input.trim())", "input.trim().length").replace("string-trim-unicode", "unicode-trim-only"),
        "unicode-checksum-only": unicode_source.replace("const preparedInput = setup(n);", "const preparedInput = setup(n).trim();").replace("hashString(input.trim())", "hashString(input)").replace("string-trim-unicode", "unicode-checksum-only"),
        "string-trim-alternating-ascii": alternating.replace("input.trim()", "input[i % 2].trim()").replace("string-trim-ascii", "string-trim-alternating-ascii"),
    }


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("label")
    parser.add_argument("--output-dir", type=Path, default=Path("target/string-trim"))
    parser.add_argument("--runtime-dir", type=Path, default=Path("target/release"))
    parser.add_argument("--only", nargs="+")
    args = parser.parse_args()
    output = args.output_dir.resolve()
    runtime = args.runtime_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime), TZ="UTC", LC_ALL="en_US.UTF-8")
    result = {
        "label": args.label,
        "sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD", "--", "crates/"])).hexdigest(),
        "node": subprocess.check_output(["node", "--version"], text=True).strip(),
        "host": platform.platform(),
        "load_start": os.getloadavg(),
        "artifacts": {name: sha256(runtime / name) for name in ["perry", "libperry_runtime.a", "libperry_stdlib.a"]},
        "benchmarks": {},
    }
    for name, source_text in sources(Path(__file__).resolve().parent).items():
        if args.only and name not in args.only:
            continue
        source = output / (name + ".ts")
        source.write_text(source_text)
        binary = output / (args.label + "-" + name)
        compiled = subprocess.run([str(runtime / "perry"), "compile", str(source), "--no-auto-optimize", "-o", str(binary)], env=env, capture_output=True, text=True)
        (output / (args.label + "-" + name + "-compile.log")).write_text(compiled.stdout + compiled.stderr)
        compiled.check_returncode()
        bench = {"source_sha256": sha256(source), "binary_sha256": sha256(binary), "node": [], "perry": []}
        for n in [100, 1000, 10000, 100000, 1000000]:
            for engine, command in [("node", ["node", str(source)]), ("perry", [str(binary)])]:
                run = subprocess.run(command + [str(n)], env=env, capture_output=True, text=True, timeout=60)
                if run.returncode:
                    raise RuntimeError((name, engine, n, run.returncode, run.stdout, run.stderr))
                row = json.loads(run.stdout)
                bench[engine].append(row)
                print(args.label, name, engine, row, flush=True)
            if bench["node"][-1]["checksum"] != bench["perry"][-1]["checksum"]:
                raise AssertionError((name, n, "checksum mismatch"))
        bench["slopes"] = {engine: slope(bench[engine]) for engine in ["node", "perry"]}
        print("slopes", name, bench["slopes"], flush=True)
        result["benchmarks"][name] = bench
        result["load_end"] = os.getloadavg()
        (output / (args.label + ".json")).write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
