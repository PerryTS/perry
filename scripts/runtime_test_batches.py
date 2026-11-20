#!/usr/bin/env python3
"""Run every runtime unit test serially, bounding test-only retained arenas.

Arena::drop deliberately keeps test-build blocks mapped (#4665). libtest gives
successive tests fresh threads, so a single full-suite process retains their
combined arenas and can fail to fork PTY children with ENOMEM. Exact-name batches
preserve the inventory and serial execution while releasing those mappings at
process exit. Full mode also preserves integration tests and doctests.
"""
import argparse
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SUMMARY = re.compile(
    r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; "
    r"(\d+) ignored; (\d+) measured; (\d+) filtered out;"
)


def inventory(output):
    names = []
    for line in output.splitlines():
        if not line.strip():
            continue
        if line.endswith(": test"):
            names.append(line[:-6])
        elif not line.endswith(": benchmark"):
            raise ValueError(f"unexpected libtest inventory line: {line!r}")
    if not names or len(names) != len(set(names)):
        raise ValueError("runtime unit-test inventory is empty or contains duplicates")
    return names


def run_batches(cargo_args=(), batch_size=128, lib_only=False, run=subprocess.run):
    if batch_size <= 0:
        raise ValueError("batch size must be positive")
    cargo = ["cargo", "test", "-p", "perry-runtime", *cargo_args]
    unit = [*cargo, "--lib"]
    listing = run([*unit, "--", "--list", "--format", "terse"],
                  cwd=ROOT, text=True, stdout=subprocess.PIPE)
    if listing.returncode:
        print(listing.stdout, flush=True)
        return listing.returncode
    names = inventory(listing.stdout)
    print(f"Runtime inventory: {len(names)} unit tests, batches of {batch_size}", flush=True)
    for offset in range(0, len(names), batch_size):
        batch = names[offset:offset + batch_size]
        print(f"Runtime tests {offset + 1}–{offset + len(batch)} / {len(names)}", flush=True)
        # A process abort has no libtest summary. Keep the active test name
        # and its diagnostics visible so a release failure is attributable.
        result = run([*unit, "--", "--exact", "--test-threads=1", "--format", "pretty", "--nocapture",
                      "--color", "never", *batch],
                     cwd=ROOT, text=True, stdout=subprocess.PIPE)
        print(result.stdout, flush=True)
        if result.returncode:
            return result.returncode
        # Subprocess fixtures can print child libtest summaries. The parent
        # reports last; only its counts cover this requested batch.
        summaries = SUMMARY.findall(result.stdout)
        if not summaries:
            raise ValueError("runtime batch did not report a test summary")
        passed, failed, ignored, measured, _ = map(int, summaries[-1])
        if failed or measured or passed + ignored != len(batch):
            raise ValueError("runtime batch did not account for every requested test")
    if not lib_only:
        for target in (["--test", "*"], ["--doc"]):
            result = run([*cargo, *target], cwd=ROOT)
            if result.returncode:
                return result.returncode
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lib", action="store_true", help="unit tests only, as in the PR tier")
    parser.add_argument("--profile")
    parser.add_argument("--jobs", type=int)
    parser.add_argument("--batch-size", type=int, default=128)
    args = parser.parse_args()
    cargo_args = []
    if args.profile:
        cargo_args += ["--profile", args.profile]
    if args.jobs:
        cargo_args += ["--jobs", str(args.jobs)]
    try:
        return run_batches(cargo_args, args.batch_size, args.lib)
    except ValueError as error:
        parser.exit(1, f"runtime test coverage error: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
