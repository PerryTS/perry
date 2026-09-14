#!/usr/bin/env python3
"""Compile checked probes, count Linux user instructions, and fold attribution."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess


PROBES = {
    "reflect_own": 200000,
    "reflect_proto": 200000,
    "iterator": 200000,
    "thenable": 20000,
    "tojson": 100000,
}
GET = re.compile(
    r"js_reflect_get|js_object_get_field_by_name|js_dynamic_object_get_property"
    r"|get_field_by_name_object_tail|native_get::"
)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, **kwargs)


def fold(stream):
    # Same root-to-leaf frame order as repro/tonly/fold.py. Preserve perf's
    # sample periods as weights, including a shortened final sample period.
    stacks = collections.Counter()
    frames = []
    period = 0
    for line in stream:
        if not line.strip():
            if frames:
                stacks[";".join(reversed(frames))] += period
            frames = []
            period = 0
        elif not line[0].isspace():
            match = re.search(r"\s(\d+)\s+instructions:u:", line)
            if not match:
                raise ValueError("unexpected perf sample header: " + line)
            period = int(match[1])
        else:
            parts = line.strip().split(None, 1)
            if len(parts) == 2:
                symbol = parts[1].rsplit(" (", 1)[0]
                frames.append(re.sub(r"\+0x[0-9a-f]+$", "", symbol))
    if frames:
        stacks[";".join(reversed(frames))] += period
    if not stacks or not sum(stacks.values()):
        raise ValueError("perf produced no instruction stacks")
    return stacks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--node", default="node")
    parser.add_argument("--smoke", action="store_true", help="compile/parity only")
    args = parser.parse_args()
    source = Path(__file__).resolve().parent
    repo = source.parents[1]
    runtime = args.runtime_dir.resolve()
    perry = runtime / "perry"
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime),
               PERRY_CACHE_DIR=str(out / "cache"), PERRY_NO_TELEMETRY="1")
    # LLVM-built runtime DWARF can exceed the host binutils reader's support.
    # Keep this override local to the run, including perf record's postlude.
    addr2line = shutil.which("llvm-addr2line-22")
    if addr2line:
        config = out / "perfconfig"
        config.write_text(f"[annotate]\naddr2line = {addr2line}\n")
        env["PERF_CONFIG"] = str(config)
    node_version = run([args.node, "--version"], capture_output=True, text=True).stdout.strip()
    if node_version.lstrip("v") != (repo / ".node-version").read_text().strip().lstrip("v"):
        raise ValueError("Node version does not match .node-version: " + node_version)
    patch = run(["git", "-C", str(repo), "diff", "--binary", "HEAD"], capture_output=True).stdout
    (out / "source.patch").write_bytes(patch)
    metadata = {
        "commit": run(["git", "-C", str(repo), "rev-parse", "HEAD"],
                      capture_output=True, text=True).stdout.strip(),
        "node": node_version,
        "source_patch_sha256": hashlib.sha256(patch).hexdigest(),
        "artifacts": {name: digest(runtime / name) for name in
                      ("perry", "libperry_runtime.a", "libperry_stdlib.a")},
        "probes": {name: digest(source / (name + ".ts")) for name in PROBES},
    }
    (out / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    results = {}
    for name, default_count in PROBES.items():
        count = 100 if args.smoke else default_count
        ts = source / (name + ".ts")
        binary = out / name
        with (out / (name + ".compile.log")).open("w") as log:
            run([str(perry), "compile", str(ts), "--no-auto-optimize", "--debug-symbols",
                 "-o", str(binary)],
                env=env, stdout=log, stderr=subprocess.STDOUT)
        expected = run([args.node, "--experimental-strip-types", str(ts), str(count)],
                       capture_output=True).stdout
        actual = run([str(binary), str(count)], env=env, capture_output=True).stdout
        if not expected or actual != expected:
            raise ValueError(f"{name}: Node {expected!r}, Perry {actual!r}")
        (out / (name + ".stdout")).write_bytes(actual)
        if args.smoke:
            print(name, "parity passed", flush=True)
            continue
        counts = []
        for repeat in range(3):
            stat = out / f"{name}.{repeat}.stat"
            run(["perf", "stat", "-x,", "-e", "instructions:u", "-o", str(stat),
                 "--", str(binary), str(count)], env=env, stdout=subprocess.DEVNULL)
            rows = [line.split(",") for line in stat.read_text().splitlines()]
            counts.append(next(int(row[0]) for row in rows if len(row) > 2
                               and row[2] == "instructions:u"))
        data = out / (name + ".perf.data")
        with (out / (name + ".record.log")).open("w") as log:
            run(["perf", "record", "--no-buildid", "--no-buildid-cache", "-m", "64M",
                 "-e", "instructions:u", "-c", "500003",
                 "--call-graph", "dwarf,16384", "-o", str(data), "--", str(binary), str(count)],
                env=env, stdout=subprocess.DEVNULL, stderr=log)
        if "lost" in (out / (name + ".record.log")).read_text().lower():
            raise ValueError(f"{name}: perf lost samples; attribution is incomplete")
        with subprocess.Popen(["perf", "script", "--no-inline", "-i", str(data)],
                              stdout=subprocess.PIPE, text=True, env=env) as process:
            stacks = fold(process.stdout)
            if process.wait() != 0:
                raise RuntimeError("perf script failed")
        with (out / (name + ".folded")).open("w") as stream:
            for stack, weight in stacks.most_common():
                stream.write(f"{weight} {stack}\n")
        total = sum(stacks.values())
        get_weight = sum(weight for stack, weight in stacks.items() if GET.search(stack))
        if not get_weight:
            raise ValueError(f"{name}: no Get frames; verify symbols and probe coverage")
        leaves = collections.Counter()
        for stack, weight in stacks.items():
            if GET.search(stack):
                leaves[stack.rsplit(";", 1)[-1]] += weight
        results[name] = {
            "iterations": count, "instructions": counts,
            "median": statistics.median(counts),
            "binary_sha256": digest(binary), "sampled_instructions": total,
            "get_inclusive_percent": 100 * get_weight / total,
            "get_leaf_percent_of_program": [(leaf, 100 * weight / total)
                                            for leaf, weight in leaves.most_common(20)],
        }
        (out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(name, results[name]["median"], results[name]["get_inclusive_percent"], flush=True)


if __name__ == "__main__":
    main()
