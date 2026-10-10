#!/usr/bin/env python3
"""Linux-only, isolated missread lane reproduction (no compiler builds)."""
import argparse
import json
import os
import re
from pathlib import Path
import statistics
import subprocess

PROGRAMS = {
    "property-prototype": ("property-prototype.ts", [], "effect"),
    "schema-record": ("schema-record.ts", [], "effect"),
    "effect": ("main.ts", [], "effect"),
    "micro": ("benchmarks/read_miss_shape.ts", ["200000"], "src"),
    "hello": ("hello.ts", [], "realprog"),
    "qsparse": ("qs/parse_nested.ts", ["20000", "1000"], "realprog/pk"),
    "fastify": ("fastify/inject.ts", ["5000", "300"], "realprog/pk"),
    "tsc": ("tscwork.ts", ["3"], "realprog"),
    "qsstr": ("qs/stringify_nested.ts", ["20000", "1000"], "realprog/pk"),
    "commander": ("commander/parse_argv.ts", ["5000", "200"], "realprog/pk"),
    "zod5k": ("zodwork.ts", ["5000"], "realprog"),
    "buffer_heavy": ("buffer_heavy.ts", [], "drivers"),
    "worker_heavy": ("worker_heavy.ts", [], "drivers"),
}
NODE = ["node", "--disable-warning=MODULE_TYPELESS_PACKAGE_JSON", "--experimental-strip-types"]


def environment(root, arm):
    src = root / ("main-src" if arm == "main" else "src")
    target = root / ("main-target" if arm == "main" else "target")
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), PERRY_RUNTIME_DIR=str(target / "release"),
               PERRY_WORKSPACE_ROOT=str(src), RUST_TEST_THREADS="1", CARGO_BUILD_JOBS="8",
               RAYON_NUM_THREADS="8", PERRY_LL_RS4GC_MAX_INSTRS="2097152",
               PERRY_KEEP_SYMBOLS="1", PERRY_NO_CACHE="1", PERRY_ALLOW_PERRY_FEATURES="1",
               PERRY_CACHE_DIR=str(root / "compile-cache" / arm),
               TMPDIR=str(root / "tmp"))
    return src, target, env


def run(cmd, cwd, env, log):
    p = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=2700)
    log.with_suffix(log.suffix + ".out").write_bytes(p.stdout)
    log.with_suffix(log.suffix + ".err").write_bytes(p.stderr)
    if p.returncode:
        raise RuntimeError(f"exit {p.returncode}: {cmd}\n{p.stderr.decode(errors='replace')[-2500:]}")
    return p.stdout


def output_result(name, output):
    if name in ("property-prototype", "schema-record"):
        return re.sub(rb"^ms [0-9.]+\n", b"", output, flags=re.M)
    if name != "effect":
        return output
    # The supplied workload prints timings alongside its functional result.
    match = re.fullmatch(rb"construct2000=\d+ms decode20000=\d+ms ok=(\d+)\n", output)
    if match is None:
        raise ValueError("unexpected Effect output")
    return b"ok=" + match[1] + b"\n"


def compile_arm(root, arm, names, diag=False, check_only=False):
    src, target, env = environment(root, arm)
    out = root / "measure" / (arm + ("-diag" if diag else ""))
    out.mkdir(parents=True, exist_ok=True)
    if diag:
        env["PERRY_IC_DIAG"] = "stderr"
    status_file = out / "status.json"
    status = json.loads(status_file.read_text()) if status_file.exists() else {}
    if not check_only:
        # A previous executable may still exist while its replacement builds.
        # Do not expose an earlier PASS as evidence for the current compile.
        status.update({name: "PENDING" for name in names})
        status_file.write_text(json.dumps(status, indent=2))
    for name in names:
        relative, args, cwd_name = PROGRAMS[name]
        cwd = root / cwd_name
        binary = out / name
        try:
            if not check_only:
                run([str(target / "release/perry"), "compile", relative, "-o", str(binary)],
                    cwd, env, out / f"{name}.compile")
                compile_errors = (out / f"{name}.compile.err").read_text()
                if "auto-optimized runtime build failed" in compile_errors:
                    raise RuntimeError("specialized runtime build failed; exclude fallback ELF from A/B")
            expected = run([*NODE, relative, *args], cwd, environment(root, arm)[2], out / f"{name}.node")
            runtime_env = environment(root, arm)[2]
            runtime_env["PERRY_METHOD_SITE_STATS"] = "1"
            if diag:
                runtime_env["PERRY_IC_DIAG"] = str(out / f"{name}.ic.txt")
            actual = run([str(binary), *args], cwd, runtime_env, out / f"{name}.perry")
            status[name] = "PASS" if output_result(name, actual) == output_result(name, expected) else "OUTPUT MISMATCH"
        except Exception as exc:
            status[name] = str(exc)
        status_file.write_text(json.dumps(status, indent=2))
        print(f"{arm}/{name}: {status[name]}", flush=True)


def stat(root, arm, name, trial, node=False, thp_off=False):
    relative, args, cwd_name = PROGRAMS[name]
    _, _, env = environment(root, arm)
    out = root / ("measure/trials-thp-off" if thp_off else "measure/trials")
    out.mkdir(parents=True, exist_ok=True)
    label = f"{name}.{trial}.{'node' if node else arm}"
    counters, rss = out / f"{label}.stat", out / f"{label}.rss"
    command = [*NODE, relative] if node else [str(root / "measure" / arm / name)]
    actual = run(["perf", "stat", "-x", ";", "-e", "instructions:u", "-o", str(counters),
                  "/usr/bin/time", "-f", "%M", "-o", str(rss), *command, *args],
                 root / cwd_name, env, out / label)
    expected = (root / "measure/main" / f"{name}.node.out").read_bytes()
    if output_result(name, actual) != output_result(name, expected):
        raise RuntimeError(f"{label}: output differs")
    count = next(int(line.split(";")[0]) for line in counters.read_text().splitlines()
                 if ";instructions:u;" in line)
    return {"instructions": count, "rss_kb": int(rss.read_text().strip())}


def compare(root, names, thp_off=False):
    if thp_off:
        import ctypes
        if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0) != 0:
            raise OSError(ctypes.get_errno(), "PR_SET_THP_DISABLE")
    path = root / ("measure/results-thp-off.json" if thp_off else "measure/results.json")
    result = json.loads(path.read_text()) if path.exists() else {}
    for name in names:
        rows = {arm: [] for arm in ["main", "head", "node"]}
        for trial in range(5):
            order = ["main", "head", "node"] if trial % 2 == 0 else ["node", "head", "main"]
            for arm in order:
                rows[arm].append(stat(root, "main" if arm == "node" else arm, name, trial,
                                      arm == "node", thp_off))
        result[name] = {"trials": rows, "medians": {
            arm: {key: statistics.median(row[key] for row in values) for key in values[0]}
            for arm, values in rows.items()}}
        path.write_text(json.dumps(result, indent=2))
        print(name, result[name]["medians"], flush=True)


def gc(root, arm, names):
    _, _, env = environment(root, arm)
    env.update(PERRY_GC_DIAG="1", PERRY_GC_TRACE="1")
    out = root / "measure" / arm
    for name in names:
        _, args, cwd = PROGRAMS[name]
        actual = run([str(out / name), *args], root / cwd, env, out / f"{name}.gc")
        expected = (root / "measure/main" / f"{name}.node.out").read_bytes()
        if output_result(name, actual) != output_result(name, expected):
            raise RuntimeError(f"{arm}/{name}: GC run output differs")
        print(f"GC {arm}/{name}: PASS", flush=True)


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--hostdir", type=Path, required=True)
    p.add_argument("--programs", default=",".join(PROGRAMS))
    p.add_argument("--thp-off", action="store_true")
    p.add_argument("action", choices=["compile-main", "compile-head", "diag-main", "diag-head",
                                      "check-main", "check-head", "checkdiag-main", "checkdiag-head",
                                      "gc-main", "gc-head", "compare"])
    opts = p.parse_args()
    (opts.hostdir / "tmp").mkdir(parents=True, exist_ok=True)
    names = opts.programs.split(",")
    if opts.action == "compare":
        compare(opts.hostdir, names, opts.thp_off)
    elif opts.action.startswith("gc-"):
        gc(opts.hostdir, opts.action.split("-")[1], names)
    else:
        compile_arm(opts.hostdir, opts.action.split("-")[1], names,
                    "diag" in opts.action, opts.action.startswith("check"))
