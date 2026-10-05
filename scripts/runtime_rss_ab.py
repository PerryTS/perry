#!/usr/bin/env python3
"""Compare Linux runtime RSS with equal executable page-cache preparation.

Example: runtime_rss_ab.py --main main/app --head head/app --cwd fixtures \
    --out results.json --expected-output node.out -- 1

Freshly linked, copied, and objcopy-rewritten ELF files can have different
cached read-ahead extents. Even identical binaries then acquire different
clean file RSS. Reset only these two files (never the host's global cache),
then read them identically before the interleaved runs. --cache existing is
available for an identical-binary control of this effect.
"""

import argparse
import ctypes
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile


def prepare_executable(path):
    with path.open("rb") as source:
        os.fsync(source.fileno())  # DONTNEED leaves dirty pages cached.
        os.posix_fadvise(source.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
        while source.read(1024 * 1024):
            pass


def disable_thp():
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(41, 1, 0, 0, 0):  # PR_SET_THP_DISABLE, per process.
        raise OSError(ctypes.get_errno(), "PR_SET_THP_DISABLE")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--main", type=Path, required=True)
    parser.add_argument("--head", type=Path, required=True)
    parser.add_argument("--cwd", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expected-output", type=Path)
    parser.add_argument("--reps", type=int, default=7)
    parser.add_argument("--cache", choices=["prepared", "existing"], default="prepared")
    parser.add_argument("args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.reps < 1:
        parser.error("--reps must be positive")
    args.out = args.out.resolve()
    args.cwd = args.cwd.resolve()
    program_args = args.args[1:] if args.args[:1] == ["--"] else args.args
    paths = {"main": args.main.resolve(), "head": args.head.resolve()}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    if args.cache == "prepared":
        for path in paths.values():
            prepare_executable(path)
    oracle = args.expected_output.read_bytes() if args.expected_output else None
    rows = []
    with tempfile.TemporaryDirectory(prefix="rss-ab-", dir=args.out.parent) as scratch:
        rss_file = Path(scratch) / "rss"
        for off in [False, True]:
            for trial in range(args.reps):
                order = ["main", "head"] if trial % 2 == 0 else ["head", "main"]
                for arm in order:
                    result = subprocess.run(
                        ["/usr/bin/time", "-f", "%M", "-o", str(rss_file),
                         str(paths[arm]), *program_args],
                        cwd=args.cwd, capture_output=True, check=True,
                        preexec_fn=disable_thp if off else None,
                    )
                    if oracle is None:
                        oracle = result.stdout
                    if result.stdout != oracle:
                        raise RuntimeError(f"{arm}: output differs on trial {trial}")
                    row = dict(arm=arm, thp_off=off, trial=trial,
                               rss_kib=int(rss_file.read_text()))
                    rows.append(row)
                    print(json.dumps(row), flush=True)
    summaries = []
    for off in [False, True]:
        for arm in paths:
            values = [r["rss_kib"] for r in rows if r["arm"] == arm and r["thp_off"] == off]
            summaries.append(dict(arm=arm, thp_off=off, min_kib=min(values),
                                  mean_kib=statistics.mean(values),
                                  median_kib=statistics.median(values), max_kib=max(values)))
    paired_deltas = []
    for off in [False, True]:
        values = {arm: [r["rss_kib"] for r in rows if r["arm"] == arm and r["thp_off"] == off]
                  for arm in paths}
        differences = [head - main for main, head in zip(values["main"], values["head"])]
        paired_deltas.append(dict(thp_off=off, samples_kib=differences,
                                  mean_kib=statistics.mean(differences),
                                  median_kib=statistics.median(differences)))
    args.out.write_text(json.dumps(dict(cache=args.cache, paths={k: str(v) for k, v in paths.items()},
                                        args=program_args, rows=rows, summaries=summaries,
                                        paired_deltas=paired_deltas), indent=2) + "\n")
    print(json.dumps(summaries, indent=2))
    print(json.dumps(paired_deltas, indent=2))


if __name__ == "__main__":
    main()
