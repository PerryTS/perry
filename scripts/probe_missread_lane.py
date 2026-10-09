#!/usr/bin/env python3
"""Collect external per-site counts from an accepted symbol-preserving binary.

Diagnostic binaries are for causality, never for the instructions/RSS A/B.
The parent is pinned to CPUs 0-55 with ASLR disabled by the caller.
"""
import argparse
from pathlib import Path
import shlex
import subprocess
from perf_missread_lane import PROGRAMS, environment

p = argparse.ArgumentParser()
p.add_argument("--hostdir", type=Path, required=True)
p.add_argument("--arm", choices=["main", "head"], required=True)
p.add_argument("--programs", default="qsparse,fastify,tsc")
opts = p.parse_args()
root = opts.hostdir.resolve()
template = Path(__file__).with_name("count_missread.bt.in").read_text()
for name in opts.programs.split(","):
    _, args, cwd = PROGRAMS[name]
    binary = root / "measure" / (opts.arm + "-diag") / name
    script = root / f"{name}.{opts.arm}.bt"
    script.write_text(template.replace("@BINARY@", str(binary)))
    command = shlex.join(["env", "PERRY_METHOD_SITE_STATS=1",
                          f"PERRY_IC_DIAG={root / (name + '.' + opts.arm + '.ic.txt')}",
                          str(binary), *args])
    _, _, env = environment(root, opts.arm)
    label = "before" if opts.arm == "main" else "after"
    with (root / f"{name}.uprobes.{label}.txt").open("w") as out, \
            (root / f"{name}.uprobes.{label}.err").open("w") as err:
        result = subprocess.run(["bpftrace", "-B", "line", "-c", command, str(script)],
                                cwd=root / cwd, env=env, stdout=out, stderr=err, timeout=2700)
    if result.returncode:
        raise RuntimeError(f"{name}: bpftrace exit {result.returncode}")
    print(f"{opts.arm}/{name}: probes complete", flush=True)
