#!/usr/bin/env python3
"""Prove the reclaim corpus exercised productive, non-retaining old GC.

Run the compiled fixture with stock GC policy, preserving stdout and the raw
PERRY_GC_DIAG stream. Never use a timing from this instrumented run as a benchmark.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def fields(line):
    return dict(re.findall(r"(\w+)=([^\s]+)", line))


def coverage(text):
    counts = dict(full=0, triggers=0, old_full=0, nonretaining_old_full=0,
                  productive_nonretaining_old_full=0, freed_bytes=0,
                  confirmed_old_reclaims=0, reclaimed_old_min_bytes=0,
                  max_old_reclaimable=0)
    trigger = None
    eligible = False
    old_before = None
    awaiting_baseline = None
    for line in text.splitlines():
        f = fields(line)
        if line.startswith("[gc-trigger] "):
            counts["triggers"] += 1
            # Missing/changed fields are a format error, never an observed zero.
            for key in ("old_reclaimable", "old_threshold", "old_baseline"):
                if key not in f or not f[key].isdigit():
                    raise ValueError("GC diagnostic missing integer " + key)
            if f.get("retaining") not in ("true", "false"):
                raise ValueError("GC diagnostic missing retaining decision")
            if awaiting_baseline is not None:
                # finish_full_old_reclaim_baseline records surviving old bytes;
                # subsequent promotion credits only increase this value. Thus
                # the drop is a lower bound on OLD reclaim, unlike freed_bytes
                # which also includes nursery garbage. Inspect the first next
                # decision only, before another collection can be attributed.
                reclaimed = max(0, awaiting_baseline - int(f["old_baseline"]))
                counts["confirmed_old_reclaims"] += int(reclaimed >= 1024 * 1024)
                counts["reclaimed_old_min_bytes"] += reclaimed
                awaiting_baseline = None
            trigger = f
            counts["max_old_reclaimable"] = max(
                counts["max_old_reclaimable"], int(f["old_reclaimable"]))
        elif line.startswith("[gc-full] "):
            counts["full"] += 1
            old = f.get("trigger") == "OldGenBytes"
            counts["old_full"] += int(old)
            eligible = bool(old and trigger and trigger.get("kind") == "OldReclaim"
                            and trigger["retaining"] == "false"
                            and int(trigger["old_reclaimable"]) >=
                            int(trigger["old_threshold"]) >= 48 * 1024 * 1024)
            counts["nonretaining_old_full"] += int(eligible)
            old_before = int(trigger["old_reclaimable"]) if eligible else None
            awaiting_baseline = None
            trigger = None
        elif line.startswith("[gc] blocks:") and eligible:
            if not f.get("freed_bytes", "").isdigit():
                raise ValueError("GC sweep diagnostic missing freed_bytes")
            freed = int(f["freed_bytes"])
            counts["freed_bytes"] += freed
            counts["productive_nonretaining_old_full"] += int(freed >= 1024 * 1024)
            awaiting_baseline = old_before if freed >= 1024 * 1024 else None
            eligible = False
    missing = [key for key in ("full", "triggers", "old_full",
               "nonretaining_old_full", "productive_nonretaining_old_full",
               "confirmed_old_reclaims")
               if counts[key] < 1]
    if missing:
        raise ValueError("GC COVERAGE FAIL: required >=1 " + ", ".join(missing)
                         + "; observed " + json.dumps(counts, sort_keys=True))
    return counts


def expected(slots, rounds):
    checksum = sum(2047 + 1048576 + 65 + ((r + i) % 26)
                   for r in range(rounds) for i in range(slots))
    checksum += sum(1048576 + 65 + ((rounds - 1 + i) % 26)
                    for i in range(slots))
    return f"reclaim {slots} {rounds} {checksum}"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("binary", nargs="?")
    ap.add_argument("--out-dir", type=Path)
    ap.add_argument("--check-log", type=Path)
    ap.add_argument("--slots", type=int, default=64)
    ap.add_argument("--rounds", type=int, default=6)
    ap.add_argument("--timeout", type=float, default=30)
    a = ap.parse_args()
    try:
        if a.check_log:
            print(json.dumps(coverage(a.check_log.read_text()), sort_keys=True))
            return 0
        if not a.binary or not a.out_dir or min(a.slots, a.rounds) < 1:
            ap.error("binary, --out-dir, and positive sizes are required")
        a.out_dir.mkdir(parents=True, exist_ok=True)
        (a.out_dir / "coverage.json").unlink(missing_ok=True)
        # Refuse a hidden stress/threshold override. The witness is about stock
        # pacing, so accepting an inherited knob would change its claim.
        overrides = [k for k in os.environ if k.startswith("PERRY_GC_")
                     or k in ("PERRY_GEN_GC", "PERRY_WRITE_BARRIERS")]
        if overrides:
            raise ValueError("remove GC overrides before the stock-policy witness: "
                             + ", ".join(sorted(overrides)))
        env = dict(os.environ, PERRY_GC_DIAG="1")
        start = time.monotonic()
        with (a.out_dir / "stdout.txt").open("w") as out, \
             (a.out_dir / "gc-diag.txt").open("w") as err:
            run = subprocess.run([str(Path(a.binary).resolve()), str(a.slots),
                                  str(a.rounds)], stdout=out, stderr=err,
                                 env=env, timeout=a.timeout)
        if run.returncode:
            raise ValueError(f"fixture exited {run.returncode}; no coverage verdict")
        got = (a.out_dir / "stdout.txt").read_text().strip()
        want = expected(a.slots, a.rounds)
        if got != want:
            raise ValueError(f"fixture checksum mismatch: {got!r} != {want!r}")
        result = coverage((a.out_dir / "gc-diag.txt").read_text())
        with open(a.binary, "rb") as binary_file:
            binary_hash = hashlib.file_digest(binary_file, "sha256").hexdigest()
        result.update(wall_seconds=round(time.monotonic() - start, 3), stdout=got,
                      binary_sha256=binary_hash)
        (a.out_dir / "coverage.json").write_text(json.dumps(result, indent=2) + "\n")
        print("GC COVERAGE PASS: " + json.dumps(result, sort_keys=True))
        return 0
    except subprocess.TimeoutExpired:
        print("DEFERRED: fixture timed out; no coverage verdict", file=sys.stderr)
        return 2
    except (ValueError, OSError) as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
