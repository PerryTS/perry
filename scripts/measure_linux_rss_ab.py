#!/usr/bin/env python3
"""Compare Linux executable arms with equal file-cache warming and GC receipts.

Manifest: {"arms": {"base": {"source": "/src/base", "target": "/tmp/base",
"runtime": "/tmp/base/release"}, "head": {...}}, "workloads": [{"name": "hello",
"cwd": "/bench", "oracle": "/bench/hello.node.out",
"binaries": {"base": "/bench/base/run", "head": "/bench/head/run"}}]}.
Paths must be absolute. Build both arms coherently before running this script.
RSS is measured in a separate direct execution, so perf cannot dominate it.
--cycles requires a quiet physical core before and during every pair.
"""
import argparse
import contextlib
import signal
import hashlib
import json
import os
from pathlib import Path
import random
import re
import statistics
import subprocess
import time


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


class QuietCoreUnavailable(RuntimeError):
    pass


def idle_samples(raw):
    rows = {}
    for line in raw.splitlines():
        fields = line.split()
        if len(fields) < 12:
            continue
        index = 2 if len(fields) > 2 and fields[1] in ("AM", "PM") else 1
        if not fields[index].isdigit():
            continue
        try:
            idle = float(fields[-1])
        except ValueError:
            continue
        rows.setdefault(int(fields[index]), []).append(idle)
    return rows


def siblings(cpu):
    value = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list").read_text()
    result = []
    for item in value.strip().split(","):
        if "-" in item:
            start, end = map(int, item.split("-"))
            result.extend(range(start, end + 1))
        else:
            result.append(int(item))
    return result


def require_quiet(cpu, directory, label, seconds):
    selected = siblings(cpu)
    result = subprocess.run(
        ["mpstat", "-P", ",".join(map(str, selected)), "1", str(seconds)],
        capture_output=True, text=True, check=True, timeout=seconds + 10,
        env=os.environ | {"LC_ALL": "C", "S_TIME_FORMAT": "ISO"})
    (directory / f"{label}.mpstat.txt").write_text(result.stdout)
    samples = idle_samples(result.stdout)
    bad = {c: min(samples.get(c, [0])) for c in selected
           if min(samples.get(c, [0])) < 98.0}
    if bad:
        raise QuietCoreUnavailable(
            f"CPU {cpu} has no quiet physical core: minimum idle {bad}; "
            "need >=98% idle on every SMT sibling. Use a quiet host/core.")


@contextlib.contextmanager
def monitor_siblings(cpu, path):
    peers = [c for c in siblings(cpu) if c != cpu]
    if not peers:
        yield
        return
    with path.open("w") as log:
        process = subprocess.Popen(
            ["mpstat", "-P", ",".join(map(str, peers)), "1"],
            stdout=log, stderr=log,
            env=os.environ | {"LC_ALL": "C", "S_TIME_FORMAT": "ISO"})
        try:
            yield
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
            process.wait(timeout=10)
    # Very short workloads have no 1-second sample; their pre-pair survey
    # remains the available control. Keep the raw monitor output either way.
    samples = idle_samples(path.read_text())
    bad = {c: min(samples[c]) for c in peers
           if c in samples and min(samples[c]) < 98.0}
    if bad:
        raise QuietCoreUnavailable(f"SMT interference during workload: {bad}")


def counter_probe(directory, extra_events):
    requested = ["cycles:u", "instructions:u", "L1-dcache-load-misses:u",
                 "LLC-load-misses:u", "stalled-cycles-backend:u"]
    requested += [event for event in extra_events.split(",") if event]
    selected, unsupported = [], []
    for index, event in enumerate(dict.fromkeys(requested)):
        path = directory / f"probe.{index}.perf"
        with Path(str(path) + ".err").open("w") as error:
            subprocess.run(["perf", "stat", "-x", ";", "-e", event,
                            "-o", str(path), "--", "/usr/bin/true"],
                           stdout=subprocess.DEVNULL, stderr=error, timeout=15)
        lines = [line.split(";") for line in path.read_text().splitlines()]
        if any(len(fields) > 2 and fields[2] == event and
               re.fullmatch(r"[0-9.]+", fields[0]) for fields in lines):
            selected.append(event)
        else:
            unsupported.append(event)
    if not {"cycles:u", "instructions:u"}.issubset(selected):
        raise RuntimeError("cycles/instructions counters unavailable")
    write_json(directory / "counter-support.json",
               dict(selected=selected, unsupported=unsupported))
    return selected + ["task-clock", "context-switches", "cpu-migrations"]


def gc_counts(diag):
    return dict(
        fulls=(len(re.findall(r"^\[gc-full\]", diag, re.M)) +
               len(re.findall(r"^\[gc-budgeted\] start .*kind=full\b", diag, re.M))),
        minors=len(re.findall(r"^\[gc-copy-minor\] ran", diag, re.M)),
        promoted_bytes=sum(map(int, re.findall(
            r"^\[gc-copy-minor\] ran[^\n]*\bpromoted_bytes=(\d+)", diag, re.M))))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--timeout", type=int, default=240)
    parser.add_argument("--cycles", action="store_true")
    parser.add_argument("--resume", action="store_true",
                        help="resume complete pairs after a rejected quiet-core window")
    parser.add_argument("--extra-events", default="", help="optional PMU events; unsupported events are recorded")
    args = parser.parse_args()
    if args.runs < 10:
        parser.error("--runs must be at least 10 for an RSS comparison")
    if args.cycles and args.runs < 15:
        parser.error("--cycles requires at least 15 pairs")
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
    previous = []
    ids = {w["name"]: {a: identity(Path(w["binaries"][a])) for a in arms}
           for w in workloads}
    saved_manifest = args.output / "manifest.json"
    attempt = 0
    if saved_manifest.exists():
        if not args.resume:
            parser.error("output already exists; choose a fresh directory or --resume")
        if json.loads(saved_manifest.read_text()) != manifest:
            parser.error("resume manifest changed")
        if json.loads((args.output / "identity.json").read_text()) != ids:
            parser.error("resume binary identity changed")
        old_host = json.loads((args.output / "host.json").read_text())
        if (old_host["cpu"], old_host["runs"], old_host["require_quiet_physical_core"]) != (args.cpu, args.runs, args.cycles):
            parser.error("resume measurement configuration changed")
        if old_host.get("harness_sha256") != identity(Path(__file__))["sha256"]:
            parser.error("resume harness changed")
        attempt = old_host.get("attempt", 0) + 1
        saved_runs = args.output / "runs.json"
        if saved_runs.exists():
            previous = json.loads(saved_runs.read_text())
        # Reject an interrupted half-pair; never combine arms from different
        # windows. Raw files survive under their unique attempt suffix.
        pairs = {}
        for row in previous:
            pairs.setdefault((row["round"], row["name"], row["mode"]), []).append(row)
        previous = [row for pair in pairs.values()
                    if len(pair) == 2 and {r["arm"] for r in pair} == set(arms)
                    for row in pair]
    write_json(saved_manifest, manifest)
    write_json(args.output / "identity.json", ids)
    oracle_ids = {w["name"]: identity(Path(w["oracle"])) for w in workloads}
    oracle_path = args.output / "oracle_identity.json"
    if args.resume and oracle_path.exists() and json.loads(oracle_path.read_text()) != oracle_ids:
        parser.error("resume oracle identity changed")
    write_json(oracle_path, oracle_ids)
    events = counter_probe(args.output, args.extra_events) if args.cycles else ["instructions:u"]
    if args.resume and saved_manifest.exists() and attempt > 0:
        if old_host["perf_events"] != events:
            parser.error("resume counter configuration changed")
    write_json(args.output / "host.json",
               dict(uname=list(os.uname()), cpu=args.cpu,
                    affinity=sorted(os.sched_getaffinity(0)),
                    cache_protocol="read both complete ELF files before each pair",
                    perf_events=events, runs=args.runs,
                    rss_protocol="separate direct execution; separate GC receipts",
                    require_quiet_physical_core=args.cycles, attempt=attempt,
                    harness_sha256=identity(Path(__file__))["sha256"]))
    if args.cycles:
        require_quiet(args.cpu, args.output, f"initial.attempt{attempt}", 10)
    rows = previous
    completed = {(r["round"], r["name"], r["mode"]) for r in rows}
    write_json(args.output / "runs.json", rows)
    for round_number in range(args.runs):
        modes = ["on", "off"] if round_number % 2 == 0 else ["off", "on"]
        order = workloads if round_number % 2 == 0 else workloads[::-1]
        for mode in modes:
            for work in order:
                if (round_number, work["name"], mode) in completed:
                    continue
                # Do not flush the shared host's page cache or change global THP.
                for binary in work["binaries"].values():
                    warm(Path(binary))
                oracle = Path(work["oracle"]).read_bytes()
                if args.cycles:
                    require_quiet(args.cpu, args.output, f"{work["name"]}.{mode}.{round_number}.attempt{attempt}.before", 1)
                arm_order = ["base", "head"] if args.cycles or round_number % 2 == 0 else ["head", "base"]
                for arm_name in arm_order:
                    arm = arms[arm_name]
                    stem = args.output / f'{work["name"]}.{mode}.{round_number}.{arm_name}.attempt{attempt}'
                    env = dict(PATH="/usr/local/bin:/usr/bin:/bin",
                               LANG="C.UTF-8", LC_ALL="C.UTF-8",
                               PERRY_NO_TELEMETRY="1", PERRY_GC_DIAG="1",
                               MIMALLOC_ALLOW_THP="1" if mode == "on" else "0",
                               CARGO_TARGET_DIR=arm["target"],
                               PERRY_RUNTIME_DIR=arm["runtime"],
                               PERRY_WORKSPACE_ROOT=arm["source"])
                    command = ["taskset", "-c", str(args.cpu), "perf", "stat",
                               "-x", ";", "-e", ",".join(events),
                               "-o", str(stem) + ".perf", "--", work["binaries"][arm_name]]
                    with Path(str(stem) + ".out").open("wb") as stdout, \
                            Path(str(stem) + ".err").open("wb") as stderr:
                        guard = (monitor_siblings(args.cpu, Path(str(stem) + ".during.mpstat.txt"))
                                 if args.cycles else contextlib.nullcontext())
                        with guard:
                            perf_start = time.monotonic()
                            subprocess.run(command, cwd=work["cwd"], env=env,
                                           stdout=stdout, stderr=stderr,
                                           timeout=args.timeout, check=True)
                            perf_wall_seconds = time.monotonic() - perf_start
                    if Path(str(stem) + ".out").read_bytes() != oracle:
                        raise AssertionError(f"{stem}: output differs from Node oracle")
                    counts = {}
                    for line in Path(str(stem) + ".perf").read_text().splitlines():
                        fields = line.split(";")
                        if len(fields) > 2 and re.fullmatch(r"[0-9.]+", fields[0]):
                            counts[fields[2]] = float(fields[0])
                            if fields[2] in events and len(fields) > 4 and float(fields[4]) < 99.0:
                                raise RuntimeError(f"{stem}: multiplexed counter {fields[2]}")
                    if any(event not in counts for event in events):
                        raise RuntimeError(f"{stem}: missing PMU counter")
                    diag = Path(str(stem) + ".err").read_text()
                    if args.cycles:
                        if counts["cycles:u"] <= 0:
                            raise RuntimeError(f"{stem}: invalid cycles")
                        clock = counts["task-clock"] / 1000
                        # perf's CSV omits the elapsed footer on this host.
                        # Measure the same PMU invocation directly; never infer
                        # its scheduling share from the separate RSS execution.
                        cpu_share = clock / perf_wall_seconds
                        if clock >= .25 and cpu_share < .98:
                            raise QuietCoreUnavailable(f"{stem}: scheduled CPU share {cpu_share:.3f}")

                    # GNU time outside perf measures the profiler's footprint too.
                    # Measure workload RSS directly in a separate execution. Keep
                    # both GC receipts: tsc collection counts can differ by run.
                    rss_command = ["/usr/bin/time", "-f", "%U %S %e %M",
                                   "-o", str(stem) + ".time", "taskset", "-c",
                                   str(args.cpu), work["binaries"][arm_name]]
                    with Path(str(stem) + ".rss.out").open("wb") as stdout, \
                            Path(str(stem) + ".rss.err").open("wb") as stderr:
                        subprocess.run(rss_command, cwd=work["cwd"], env=env,
                                       stdout=stdout, stderr=stderr,
                                       timeout=args.timeout, check=True)
                    if Path(str(stem) + ".rss.out").read_bytes() != oracle:
                        raise AssertionError(f"{stem}: RSS run differs from Node oracle")
                    user, system, wall, rss = Path(str(stem) + ".time").read_text().split()
                    row = dict(round=round_number, name=work["name"], mode=mode,
                               attempt=attempt, receipt=str(stem),
                               arm=arm_name, instructions=int(counts["instructions:u"]),
                               cpu_seconds=float(user) + float(system),
                               wall_seconds=float(wall), rss_kib=int(rss),
                               size=ids[work["name"]][arm_name]["bytes"],
                               **gc_counts(diag),
                               rss_gc=gc_counts(Path(str(stem) + ".rss.err").read_text()))
                    if args.cycles:
                        row.update(cycles=int(counts["cycles:u"]),
                                   ipc=counts["instructions:u"] / counts["cycles:u"],
                                   perf_wall_seconds=perf_wall_seconds,
                                   scheduled_cpu_share=cpu_share, pmu=counts)
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
                    for key in (("instructions", "cpu_seconds", "wall_seconds",
                                 "rss_kib", "size", "promoted_bytes") +
                                (("cycles", "ipc") if args.cycles else ()))}
                medians[arm]["rss_range_kib"] = [
                    min(r["rss_kib"] for r in group), max(r["rss_kib"] for r in group)]
                medians[arm]["fulls"] = [r["fulls"] for r in group]
                medians[arm]["minors"] = [r["minors"] for r in group]
                medians[arm]["rss_gc"] = [r["rss_gc"] for r in group]
                if args.cycles:
                    medians[arm]["pmu"] = {event: statistics.median(r["pmu"][event] for r in group)
                                            for event in events}
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
    try:
        main()
    except QuietCoreUnavailable as error:
        raise SystemExit(str(error)) from error
