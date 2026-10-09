#!/usr/bin/env python3
"""Sample a lane-owned child's VMAs; does not alter a system THP setting."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument("--out", type=Path, required=True)
p.add_argument("--thp-off", action="store_true")
p.add_argument("command", nargs=argparse.REMAINDER)
opts = p.parse_args()
if opts.thp_off:
    import ctypes
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), "PR_SET_THP_DISABLE")
samples = []
region_samples = []
with opts.out.with_suffix(".out").open("wb") as stdout, opts.out.with_suffix(".err").open("wb") as stderr:
    child = subprocess.Popen(opts.command, stdout=stdout, stderr=stderr, env=os.environ)
    while child.poll() is None:
        try:
            raw = Path(f"/proc/{child.pid}/smaps").read_text()
            groups = {}
            regions = []
            for block in re.split(r"(?m)^(?=[0-9a-f]+-[0-9a-f]+ )", raw):
                if not block.strip():
                    continue
                header, *lines = block.splitlines()
                parts = header.split(maxsplit=5)
                label = parts[5] if len(parts) == 6 else "anonymous"
                row = groups.setdefault(label, {"Rss": 0, "Anonymous": 0, "AnonHugePages": 0})
                region = {"path": label, "permissions": parts[1], "offset": parts[2],
                          "Rss": 0, "Anonymous": 0, "AnonHugePages": 0}
                for line in lines:
                    key, _, value = line.partition(":")
                    if key in row:
                        row[key] += int(value.split()[0])
                        region[key] = int(value.split()[0])
                regions.append(region)
            samples.append(groups)
            region_samples.append(regions)
        except (FileNotFoundError, ProcessLookupError):
            pass
        time.sleep(.02)
    if child.returncode:
        raise RuntimeError(f"child exit {child.returncode}")
peak_index = max(range(len(samples)), key=lambda i: sum(row["Rss"] for row in samples[i].values()))
opts.out.write_text(json.dumps({"peak": samples[peak_index], "samples": len(samples),
                                "peak_regions": region_samples[peak_index]}, indent=2))
