#!/usr/bin/env python3
"""Reproduce #12015 profiles and serial, interleaved instruction/RSS trials.

Run on the Linux host; all outputs stay under --hostdir. Each arm has its
own source directory and Cargo target. No compiler builds happen here.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess


PROGRAMS = {
    "tsc": ("tscwork.ts", ["1"], False),
    "zod5k": ("zodwork.ts", ["5000"], False),
    "qsparse": ("qs/parse_nested.ts", ["20000", "1000"], True),
    "qsstr": ("qs/stringify_nested.ts", ["20000", "1000"], True),
    "commander": ("commander/parse_argv.ts", ["5000", "200"], True),
    "hello": ("hello.ts", [], False),
    "fastify": ("fastify/inject.ts", ["500", "30"], True),
}
NODE = ["node", "--disable-warning=MODULE_TYPELESS_PACKAGE_JSON", "--experimental-strip-types"]
MICROS = ["keys", "values_entries", "descriptors", "for_in", "spread_assign", "method_factory"]


def environment(root, arm):
    source_name, target_name = {"main": ("main-src", "main-target"),
                                "fix1": ("fix1-src", "fix1-target")}.get(arm, ("src", "target"))
    source, target = root / source_name, root / target_name
    env = dict(os.environ)
    env.update(CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target / "release"),
               PERRY_WORKSPACE_ROOT=str(source), RUST_TEST_THREADS="1", CARGO_BUILD_JOBS="16",
               PERRY_KEEP_SYMBOLS="1", PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1",
               PERRY_SKIP_BUILD="1", PERRY_ALLOW_PERRY_FEATURES="1", TMPDIR=str(root / "tmp"))
    return source, target, env


def run(cmd, cwd, env, log=None, stderr_file=None):
    result = subprocess.run(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=1800)
    if log:
        log.write_bytes(result.stdout + result.stderr)
    if stderr_file:
        stderr_file.write_bytes(result.stderr)
    if result.returncode:
        raise RuntimeError(f"exit {result.returncode}: {cmd}\n{result.stderr.decode(errors='replace')[-4000:]}")
    return result.stdout


def compile_arm(root, arm):
    source, target, env = environment(root, arm)
    out = root / "measure" / arm
    out.mkdir(parents=True, exist_ok=True)
    for name, (relative, args, package) in PROGRAMS.items():
        if arm == "fix1" and name != "qsstr":
            continue
        cwd = root / "realprog" / ("pk" if package else "")
        binary = out / name
        run([str(target / "release/perry"), "compile", relative, "-o", str(binary)],
            cwd, env, out / f"{name}.compile.log")
        expected = run([*NODE, relative, *args], cwd, env,
                       stderr_file=out / f"{name}.node.err")
        actual = run([str(binary), *args], cwd, env, stderr_file=out / f"{name}.perry.err")
        (out / f"{name}.node.out").write_bytes(expected)
        (out / f"{name}.perry.out").write_bytes(actual)
        if (actual != expected or (out / f"{name}.node.err").read_bytes()
                != (out / f"{name}.perry.err").read_bytes()):
            raise RuntimeError(f"{arm}/{name}: Node output differs")
        print(f"{arm}/{name}: compiled, Node output matches", flush=True)
    microdir = source / "benchmarks/object_metadata_12015"
    for name in MICROS:
        run([str(target / "release/perry"), "compile", f"{name}.ts", "-o", str(out / name)],
            microdir, env, out / f"{name}.compile.log")


def stat(root, label, cmd, cwd, env):
    out = root / "measure" / "trials"
    out.mkdir(parents=True, exist_ok=True)
    counters, rss = out / f"{label}.stat", out / f"{label}.rss"
    stdout = run(["perf", "stat", "-x", ";", "-e", "instructions:u", "-o", str(counters),
                  "/usr/bin/time", "-f", "%M", "-o", str(rss), *cmd], cwd, env,
                 stderr_file=out / f"{label}.err")
    (out / f"{label}.out").write_bytes(stdout)
    instructions = next(int(line.split(";")[0]) for line in counters.read_text().splitlines()
                        if ";instructions:u;" in line)
    return {"instructions": instructions, "rss_kb": int(rss.read_text().strip())}


def profiles(root):
    _, _, env = environment(root, "main")
    out = root / "measure" / "profiles"
    out.mkdir(parents=True, exist_ok=True)
    for name, (_, args, package) in PROGRAMS.items():
        if name == "hello":
            continue
        cwd = root / "realprog" / ("pk" if package else "")
        binary = root / "measure/main" / name
        data = out / f"{name}.data"
        run(["perf", "record", "-e", "cycles:u", "-F", "1999", "-g", "-o", str(data),
             "--", str(binary), *args], cwd, env, out / f"{name}.record.log")
        (out / f"{name}.functions.txt").write_bytes(run(
            ["perf", "report", "-i", str(data), "--stdio", "--no-inline", "--no-children",
             "--call-graph", "none", "--percent-limit", "0.01", "--sort", "symbol"], cwd, env))
        (out / f"{name}.samples.txt").write_bytes(run(
            ["perf", "script", "-i", str(data), "--no-inline", "-G", "-F",
             "period,ip,sym,symoff,dso"], cwd, env))
        (out / f"{name}.stacks.txt").write_bytes(run(
            ["perf", "script", "-i", str(data), "--no-inline", "-F", "period,ip,sym,symoff,dso"], cwd, env))
        print(f"profile/{name}: recorded", flush=True)


def compare(root):
    results = {"programs": {}, "micros": {}}
    for name, (relative, args, package) in PROGRAMS.items():
        cwd = root / "realprog" / ("pk" if package else "")
        trials = {arm: [] for arm in ["main", "head"]}
        _, _, node_env = environment(root, "main")
        node_err = root / "measure/main" / f"{name}.node.err"
        run([*NODE, relative, *args], cwd, node_env,
            stderr_file=node_err)
        for trial in range(5):
            for arm in (["main", "head"] if trial % 2 == 0 else ["head", "main"]):
                _, _, env = environment(root, arm)
                row = stat(root, f"{name}.{trial}.{arm}",
                           [str(root / "measure" / arm / name), *args], cwd, env)
                trials[arm].append(row)
                expected = (root / "measure/main" / f"{name}.node.out").read_bytes()
                if (root / "measure/trials" / f"{name}.{trial}.{arm}.out").read_bytes() != expected:
                    raise RuntimeError(f"{name}/{arm}/{trial}: output differs")
                if (root / "measure/trials" / f"{name}.{trial}.{arm}.err").read_bytes() != node_err.read_bytes():
                    raise RuntimeError(f"{name}/{arm}/{trial}: stderr differs")
        summary = {arm: {field: statistics.median(row[field] for row in rows)
                         for field in ["instructions", "rss_kb"]} for arm, rows in trials.items()}
        results["programs"][name] = {"trials": trials, "medians": summary}
        (root / "measure/results.json").write_text(json.dumps(results, indent=2))
        print(name, summary, flush=True)
    for name in MICROS:
        rows = {arm: [] for arm in ["main", "head", "node"]}
        for trial in range(5):
            for arm in (["main", "head", "node"] if trial % 2 == 0 else ["node", "head", "main"]):
                source, _, env = environment(root, arm)
                cwd = source / "benchmarks/object_metadata_12015"
                cmd = ([*NODE, f"{name}.ts"] if arm == "node"
                       else [str(root / "measure" / arm / name)])
                # Two counts remove process startup; count=0 is legal.
                low = stat(root, f"micro.{name}.{trial}.{arm}.low", [*cmd, "10000"], cwd, env)
                high = stat(root, f"micro.{name}.{trial}.{arm}.high", [*cmd, "110000"], cwd, env)
                rows[arm].append((high["instructions"] - low["instructions"]) / 100000)
        for trial in range(5):
            for size in ["low", "high"]:
                expected = (root / "measure/trials" / f"micro.{name}.{trial}.node.{size}.out").read_bytes()
                for arm in ["main", "head"]:
                    actual = (root / "measure/trials" / f"micro.{name}.{trial}.{arm}.{size}.out").read_bytes()
                    if actual != expected:
                        raise RuntimeError(f"micro/{name}/{trial}/{arm}/{size}: output differs")
                    node_err = root / "measure/trials" / f"micro.{name}.{trial}.node.{size}.err"
                    actual_err = root / "measure/trials" / f"micro.{name}.{trial}.{arm}.{size}.err"
                    if actual_err.read_bytes() != node_err.read_bytes():
                        raise RuntimeError(f"micro/{name}/{trial}/{arm}/{size}: stderr differs")
        results["micros"][name] = {"trials": rows, "instructions_per_op":
                                  {arm: statistics.median(values) for arm, values in rows.items()}}
        (root / "measure/results.json").write_text(json.dumps(results, indent=2))
        print(name, results["micros"][name]["instructions_per_op"], flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hostdir", type=Path, required=True)
    parser.add_argument("action", choices=["compile-main", "compile-head", "compile-fix1", "profiles", "compare"])
    args = parser.parse_args()
    (args.hostdir / "tmp").mkdir(parents=True, exist_ok=True)
    if args.action.startswith("compile-"):
        compile_arm(args.hostdir, args.action.removeprefix("compile-"))
    elif args.action == "profiles":
        profiles(args.hostdir)
    else:
        compare(args.hostdir)
