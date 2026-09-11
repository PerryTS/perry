#!/usr/bin/env python3
"""Generic sort A/B: paired compiler/runtime directories, shared inputs, Node oracle.

Each directory must contain perry, libperry_runtime.a and libperry_stdlib.a
built together. Example:
  python3 benchmarks/array-sort/run.py --baseline /tmp/base --candidate /tmp/new
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline", type=Path, required=True)
parser.add_argument("--candidate", type=Path, required=True)
parser.add_argument("--node", default="node")
parser.add_argument("--size", type=int, default=100000)
parser.add_argument("--samples", type=int, default=7)
parser.add_argument("--warmups", type=int, default=2)
parser.add_argument("--output", type=Path)
args = parser.parse_args()
if args.size < 1 or args.samples < 1 or args.warmups < 0:
    parser.error("size and samples must be positive; warmups must be nonnegative")
out = (args.output or Path(tempfile.mkdtemp(prefix="perry-array-sort-"))).resolve()
out.mkdir(parents=True, exist_ok=True)
source = out / "bench.ts"
shutil.copyfile(Path(__file__).with_name("bench.ts"), source)
env = {k: v for k, v in os.environ.items() if not k.startswith("PERRY_")}
report = {
    "platform": platform.platform(), "size": args.size,
    "method": f"{args.warmups} warmups + {args.samples} randomized interleaved fresh-process samples per engine. performance.now measures sort only. Identical pre-generated JSON inputs; every output element, type and stable object order checked against Node on every run. Matching compiler/runtime pairs; compiler sources and build flags must be held fixed across the A/B.",
    "node": subprocess.check_output([args.node, "--version"], text=True).strip(),
    "builds": {}, "rows": [],
}
commands = {"node": [args.node, str(source)]}
for name, directory in [("baseline", args.baseline), ("candidate", args.candidate)]:
    directory = directory.resolve()
    compiler = directory / "perry"
    report["builds"][name] = {
        "directory": str(directory),
        "version": subprocess.check_output([str(compiler), "--version"], text=True).strip(),
        "sha256": {file: hashlib.sha256((directory / file).read_bytes()).hexdigest()
                   for file in ["perry", "libperry_runtime.a", "libperry_stdlib.a"]},
    }
    binary = out / name
    compile_cmd = [str(compiler), "compile", str(source), "-o", str(binary),
                   "--no-auto-optimize", "--cache-dir", str(out / "cache")]
    with (out / f"compile-{name}.log").open("w") as log:
        subprocess.run(compile_cmd, cwd=out, env={**env, "PERRY_RUNTIME_DIR": str(directory)},
                       stdout=log, stderr=subprocess.STDOUT, timeout=180, check=True)
    commands[name] = [str(binary)]

n = args.size
rng = random.Random(289)
random_values = [rng.randrange(n) for _ in range(n)]
fixtures = {
    "random": random_values,
    "sorted": list(range(n)), "reverse": list(reversed(range(n))),
    "equal": [3] * n, "duplicates": [x % 8 for x in random_values],
    "runs": [(n - 1 - (i // 137) * 137) + i % 137 for i in range(n)],
    "nearly_sorted": [random_values[i] if i % 97 == 0 else i for i in range(n)],
    "organ_pipe": [min(i, n - 1 - i) for i in range(n)],
}


def run(engine, distribution, kind, fixture):
    command = commands[engine] + [distribution, kind, str(n), "1", str(fixture)]
    result = subprocess.run(command, cwd=out, env=env, capture_output=True,
                            text=True, timeout=60, check=True)
    if result.stderr:
        raise RuntimeError(result.stderr)
    record = json.loads(result.stdout)
    elapsed = record.pop("sortMs")
    assert record["length"] == n
    return elapsed, record


for distribution, values in fixtures.items():
    fixture = out / f"input-{distribution}.json"
    fixture.write_text(json.dumps(values, separators=(",", ":")))
    for kind in ["number", "object", "string"]:
        _, expected = run("node", distribution, kind, fixture)
        # Python considers True == 1. Compare canonical JSON so a wrong JS
        # value type cannot pass the oracle check through Python equality.
        expected_json = json.dumps(expected, sort_keys=True, separators=(",", ":"))
        samples = {name: [] for name in commands}
        for iteration in range(args.warmups + args.samples):
            order = list(commands)
            rng.shuffle(order)
            for name in order:
                elapsed, record = run(name, distribution, kind, fixture)
                assert json.dumps(record, sort_keys=True, separators=(",", ":")) == expected_json, (name, distribution, kind, "output differs from Node")
                if iteration >= args.warmups:
                    samples[name].append(elapsed)
        row = {"distribution": distribution, "kind": kind,
               "output_sha256": hashlib.sha256(json.dumps(expected, separators=(",", ":")).encode()).hexdigest(),
               "timings_ms": {name: {"median": statistics.median(times), "min": min(times),
                                     "max": max(times), "samples": times}
                              for name, times in samples.items()}}
        report["rows"].append(row)
        (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
        medians = {name: round(v["median"], 4) for name, v in row["timings_ms"].items()}
        print(distribution, kind, medians, flush=True)
print("Results:", out / "results.json", flush=True)
