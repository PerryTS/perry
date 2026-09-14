"""Five alternating runtime runs of already-linked Linux benchmark binaries.

Usage: alternate.py RESULTS_DIR [hot numbers records records-hot-3200]
Each fixture must have before-FIXTURE/app and after-FIXTURE/app. One untimed
warmup per arm precedes the five measured pairs, pinned to the same CPU.
"""

import json
import os
from pathlib import Path
import statistics
import subprocess
import sys


def main():
    if os.uname().sysname != "Linux":
        raise SystemExit("build-host measurements only")
    root = Path(sys.argv[1]).resolve()
    fixtures = sys.argv[2:] or ["hot", "numbers", "records", "records-hot-3200"]
    cpu = max(os.sched_getaffinity(0))
    report = {"cpu": cpu, "warmups_per_arm": 1, "runs_per_arm": 5, "fixtures": {}}
    for fixture in fixtures:
        rows = {"before": [], "after": []}
        expected = None
        for iteration in range(6):
            for arm in rows:
                binary = root / f"{arm}-{fixture}" / "app"
                output = subprocess.check_output(
                    ["taskset", "-c", str(cpu), str(binary)], text=True,
                    cwd=binary.parent, timeout=60,
                ).strip()
                tokens = output.split()
                timings = {}
                while tokens and tokens[0].endswith("_ms"):
                    key, value, *tokens = tokens
                    timings[key] = int(value)
                if expected is None:
                    expected = tokens
                if tokens != expected or not timings:
                    raise AssertionError(f"{fixture} {arm}: bad output {output!r}")
                if iteration:
                    rows[arm].append(timings)
                print(f"{fixture} {arm} {iteration}: {output}", flush=True)
        report["fixtures"][fixture] = {
            "checksums": expected,
            "runs": rows,
            "medians_ms": {
                arm: {key: statistics.median(row[key] for row in runs)
                      for key in runs[0]}
                for arm, runs in rows.items()
            },
        }
    (root / "runtime.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
