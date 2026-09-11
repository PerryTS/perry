"""Compare builds of the unchanged #10063 workload, with serialized 60s runs."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=3)
    args = parser.parse_args()
    node = subprocess.check_output(["node", "--version"], text=True).strip()
    if node != "v26.5.1":
        parser.error(f"use the issue's Node v26.5.1 oracle, found {node}")
    if args.rounds < 1:
        parser.error("--rounds must be positive")
    source = args.source.resolve()
    expected = {100: 53207531, 1000: 509027806, 10000: 6382792,
                100000: 66481643, 1000000: 475838285}
    commands = {"node": ["node", str(source)],
                "before": [str(args.before.resolve())],
                "after": [str(args.after.resolve())]}
    report = {"node": node, "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
              "timeout_seconds": 60, "rows": []}
    env = {**os.environ, "TZ": "UTC", "LC_ALL": "en_US.UTF-8"}
    failed = False
    for repeat in range(args.rounds):
        # Alternate the native order to reduce order/thermal bias. Never run
        # two benchmark processes concurrently on the measurement host.
        engines = ["node", "before", "after"] if repeat % 2 == 0 else ["node", "after", "before"]
        for size, checksum in expected.items():
            for engine in engines:
                row = {"repeat": repeat, "engine": engine, "n": size}
                started = time.monotonic()
                try:
                    process = subprocess.run(commands[engine] + [str(size)], env=env,
                                             capture_output=True, text=True, timeout=60)
                    row["exit_code"] = process.returncode
                    if process.returncode:
                        row.update(status="error", stdout=process.stdout, stderr=process.stderr)
                        failed = True
                    else:
                        result = json.loads(process.stdout)
                        row.update(result)
                        row["status"] = "ok" if result["checksum"] == checksum else "checksum-mismatch"
                        failed |= row["status"] != "ok"
                except subprocess.TimeoutExpired:
                    row["status"] = "timeout"
                    failed |= engine != "before"
                row["wall_seconds"] = time.monotonic() - started
                report["rows"].append(row)
                args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
                print(json.dumps(row), flush=True)
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
