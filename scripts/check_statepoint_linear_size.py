#!/usr/bin/env python3
"""Compile #11926 witnesses and reject quadratic statepoint machine code.

Run on x86_64 Linux with the compiler/runtime/workspace identity explicitly
selected by the caller. --work-dir keeps IR, logs, binaries and JSON evidence.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

def witness(n):
    lines = ["function sink(x: any): any { return x; }",
             "export function big(seed: any): any {"]
    lines += [f"  let v{i}: any = sink({{k: seed, i: {i}}});" for i in range(n)]
    lines += [f"  sink(v{i});" for i in range(n)]
    lines += ["  let s = 0;"]
    lines += [f"  s += v{i}.i;" for i in range(n)]
    lines += ["  return s;", "}", "console.log(big(1));"]
    return "\n".join(lines) + "\n"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--work-dir", type=Path)
    args = parser.parse_args()
    out = args.work_dir or Path(tempfile.mkdtemp(prefix="sp-linear-size-"))
    out.mkdir(parents=True, exist_ok=True)
    rows = []
    for n in (25, 50, 100, 200):
        directory = out / f"q{n}"
        directory.mkdir(exist_ok=True)
        source = directory / f"q{n}.ts"
        source.write_text(witness(n))
        binary = directory / "q.bin"
        ir = directory / "ir"
        ir.mkdir(exist_ok=True)
        env = os.environ | {"PERRY_SAVE_LL": str(ir), "PERRY_NO_TELEMETRY": "1"}
        with (directory / "compile.log").open("w") as log:
            subprocess.run([
                "/usr/bin/time", "-f", "%U %S %e %M", "-o", str(directory / "time"),
                str(args.perry), "compile", str(source), "-o", str(binary),
                "--no-cache", "--no-auto-optimize", "--debug-symbols",
            ], env=env, stdout=log, stderr=log, check=True)
        symbols = subprocess.check_output(["nm", "-S", "--size-sort", str(binary)], text=True)
        sizes = []
        for line in symbols.splitlines():
            parts = line.split()
            if len(parts) == 4 and parts[3].startswith("perry_fn_") and parts[3].endswith("__big"):
                sizes.append(int(parts[1], 16))
        if len(sizes) != 1:
            raise AssertionError(f"q{n}: expected one ordinary big symbol, found {sizes}")
        result = subprocess.check_output([str(binary)], text=True).strip()
        if result != str(n * (n - 1) // 2):
            raise AssertionError(f"q{n}: output {result!r}")
        user, system, wall, rss = (directory / "time").read_text().split()
        sections = subprocess.check_output(["size", "-A", str(binary)], text=True)
        gcmap = next(int(line.split()[1]) for line in sections.splitlines()
                     if line.startswith(".perry_gcmap"))
        rows.append(dict(n=n, symbol_bytes=sizes[0], gcmap_bytes=gcmap, binary_bytes=binary.stat().st_size,
                         cpu_seconds=float(user) + float(system), wall_seconds=float(wall),
                         compile_rss_kib=int(rss)))
        (out / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
    print("| N | big bytes | compile CPU s | wall s | RSS KiB |")
    print("|---:|---:|---:|---:|---:|")
    for row in rows:
        print(f"| {row['n']} | {row['symbol_bytes']} | {row['cpu_seconds']:.2f} | "
              f"{row['wall_seconds']:.2f} | {row['compile_rss_kib']} |")
    failures = []
    if rows[-1]["symbol_bytes"] > 248_858:
        failures.append(f"q200 {rows[-1]['symbol_bytes']} bytes exceeds 248858")
    for smaller, larger in zip(rows, rows[1:]):
        for metric in ("symbol_bytes", "gcmap_bytes"):
            if larger[metric] > smaller[metric] * 2.5:
                failures.append(f"q{larger['n']} {metric} exceeds 2.5 times q{smaller['n']}")
    if failures:
        raise AssertionError("; ".join(failures))
    print("Linear statepoint size gate passed.")

if __name__ == "__main__":
    main()
