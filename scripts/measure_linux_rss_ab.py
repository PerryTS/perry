#!/usr/bin/env python3
"""Compare Linux executable arms with equal file-cache warming and GC receipts.

Manifest: {"arms": {"base": {"source": "/src/base", "target": "/tmp/base",
"runtime": "/tmp/base/release"}, "head": {...}}, "workloads": [{"name": "hello",
"cwd": "/bench", "oracle": "/bench/hello.node.out",
"binaries": {"base": "/bench/base/run", "head": "/bench/head/run"}}]}.
Paths must be absolute. Build both arms coherently before running this script.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import re
import statistics
import subprocess


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def warm(path):
    with path.open("rb") as stream:
        while stream.read(8 * 1024 * 1024):
            pass


def identity(path):
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    info = path.stat()
    return dict(path=str(path), sha256=digest, bytes=info.st_size,
                device=info.st_dev, inode=info.st_ino)


def median_interval(values):
    # Paired bootstrap is descriptive; it does not certify a strict upper bound.
    rng = random.Random(12023)
    samples = sorted(statistics.median(rng.choices(values, k=len(values)))
                     for _ in range(10000))
    return [samples[249], samples[9749]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--timeout", type=int, default=240)
    args = parser.parse_args()
    if args.runs < 10:
        parser.error("--runs must be at least 10 for an RSS comparison")
    if args.cpu not in os.sched_getaffinity(0):
        parser.error("--cpu is outside the current affinity")
    manifest = json.loads(args.manifest.read_text())
    workloads = manifest["workloads"]
    arms = manifest["arms"]
    if set(arms) != {"base", "head"}:
        parser.error("exactly base and head arms are required")
    names = [w["name"] for w in workloads]
    if not names or len(names) != len(set(names)):
        parser.error("workload names must be nonempty and unique")
    if any(not re.fullmatch(r"[A-Za-z0-9_-]+", name) for name in names):
        parser.error("workload names must be safe file labels")
    paths = []
    for arm in arms.values():
        paths.extend(arm[key] for key in ("source", "target", "runtime"))
    for work in workloads:
        paths.extend([work["cwd"], work["oracle"], *work["binaries"].values()])
        if set(work["binaries"]) != {"base", "head"}:
            parser.error("each workload needs both binaries")
    if any(not Path(p).is_absolute() for p in paths):
        parser.error("all manifest paths must be absolute")
    args.output.mkdir(parents=True, exist_ok=True)
    if (args.output / "runs.json").exists():
        parser.error("output already contains runs; choose a fresh directory")
    write_json(args.output / "manifest.json", manifest)
    ids = {w["name"]: {a: identity(Path(w["binaries"][a])) for a in arms}
           for w in workloads}
    write_json(args.output / "identity.json", ids)
    write_json(args.output / "oracle_identity.json",
               {w["name"]: identity(Path(w["oracle"])) for w in workloads})
    write_json(args.output / "host.json",
               dict(uname=list(os.uname()), cpu=args.cpu,
                    affinity=sorted(os.sched_getaffinity(0)),
                    cache_protocol="read both complete ELF files before each pair",
                    perf_events="instructions:u", runs=args.runs))
    rows = []
    for round_number in range(args.runs):
        modes = ["on", "off"] if round_number % 2 == 0 else ["off", "on"]
        order = workloads if round_number % 2 == 0 else workloads[::-1]
        for mode in modes:
            for work in order:
                # Do not flush the shared host's page cache or change global THP.
                for binary in work["binaries"].values():
                    warm(Path(binary))
                oracle = Path(work["oracle"]).read_bytes()
                arm_order = ["base", "head"] if round_number % 2 == 0 else ["head", "base"]
                for arm_name in arm_order:
                    arm = arms[arm_name]
                    stem = args.output / f'{work["name"]}.{mode}.{round_number}.{arm_name}'
                    env = dict(PATH="/usr/local/bin:/usr/bin:/bin",
                               LANG="C.UTF-8", LC_ALL="C.UTF-8",
                               PERRY_NO_TELEMETRY="1", PERRY_GC_DIAG="1",
                               MIMALLOC_ALLOW_THP="1" if mode == "on" else "0",
                               CARGO_TARGET_DIR=arm["target"],
                               PERRY_RUNTIME_DIR=arm["runtime"],
                               PERRY_WORKSPACE_ROOT=arm["source"])
                    command = ["/usr/bin/time", "-f", "%U %S %e %M", "-o",
                               str(stem) + ".time", "taskset", "-c", str(args.cpu),
                               "perf", "stat", "-x", ";", "-e", "instructions:u",
                               "-o", str(stem) + ".perf", "--", work["binaries"][arm_name]]
                    with Path(str(stem) + ".out").open("wb") as stdout, \
                            Path(str(stem) + ".err").open("wb") as stderr:
                        subprocess.run(command, cwd=work["cwd"], env=env,
                                       stdout=stdout, stderr=stderr,
                                       timeout=args.timeout, check=True)
                    if Path(str(stem) + ".out").read_bytes() != oracle:
                        raise AssertionError(f"{stem}: output differs from Node oracle")
                    user, system, wall, rss = Path(str(stem) + ".time").read_text().split()
                    perf = Path(str(stem) + ".perf").read_text()
                    match = re.search(r"^(\d+);[^;]*;instructions:u;", perf, re.M)
                    if match is None:
                        raise AssertionError(f"{stem}: missing instructions counter")
                    diag = Path(str(stem) + ".err").read_text()
                    row = dict(round=round_number, name=work["name"], mode=mode,
                               arm=arm_name, instructions=int(match[1]),
                               cpu_seconds=float(user) + float(system),
                               wall_seconds=float(wall), rss_kib=int(rss),
                               size=ids[work["name"]][arm_name]["bytes"],
                               fulls=(len(re.findall(r"^\[gc-full\]", diag, re.M)) +
                                      len(re.findall(r"^\[gc-budgeted\] start .*kind=full\b",
                                                     diag, re.M))),
                               minors=len(re.findall(r"^\[gc-copy-minor\] ran", diag, re.M)),
                               promoted_bytes=sum(map(int, re.findall(
                                   r"^\[gc-copy-minor\] ran[^\n]*\bpromoted_bytes=(\d+)",
                                   diag, re.M))))
                    rows.append(row)
                    write_json(args.output / "runs.json", rows)
                    print(json.dumps(row), flush=True)
    summary = []
    for name in names:
        for mode in ("on", "off"):
            groups = {a: [r for r in rows if r["name"] == name and
                          r["mode"] == mode and r["arm"] == a] for a in arms}
            paired = [h["rss_kib"] - b["rss_kib"]
                      for b, h in zip(groups["base"], groups["head"])]
            medians = {}
            for arm, group in groups.items():
                medians[arm] = {
                    key: statistics.median(r[key] for r in group)
                    for key in ("instructions", "cpu_seconds", "wall_seconds",
                                "rss_kib", "size", "promoted_bytes")}
                medians[arm]["rss_range_kib"] = [
                    min(r["rss_kib"] for r in group), max(r["rss_kib"] for r in group)]
                medians[arm]["fulls"] = [r["fulls"] for r in group]
                medians[arm]["minors"] = [r["minors"] for r in group]
            summary.append(dict(name=name, mode=mode, arms=medians,
                                paired_rss_delta_kib=paired,
                                paired_rss_median_kib=statistics.median(paired),
                                paired_rss_bootstrap95_kib=median_interval(paired)))
    write_json(args.output / "summary.json", summary)
    # Check files did not change during the run; cache warming never rewrites them.
    for work in workloads:
        for arm in arms:
            if identity(Path(work["binaries"][arm])) != ids[work["name"]][arm]:
                raise AssertionError(f'{work["name"]}/{arm}: binary identity changed')
    (args.output / "done").touch()


if __name__ == "__main__":
    main()
