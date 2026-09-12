"""Run the unchanged #10089 DataView workloads serially against Node and Perry."""

import argparse
import json
import math
import os
from pathlib import Path
import subprocess


SIZES = [100, 1000, 10000, 100000, 1000000]


def slope(points):
    if len(points) < 2:
        return None
    xs = [math.log(n) for n, _ in points]
    ys = [math.log(ms) for _, ms in points]
    mean_x = sum(xs) / len(xs)
    mean_y = sum(ys) / len(ys)
    return sum((x - mean_x) * (y - mean_y) for x, y in zip(xs, ys)) / sum(
        (x - mean_x) ** 2 for x in xs
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--set-app", type=Path, required=True)
    parser.add_argument("--get-app", type=Path, required=True)
    parser.add_argument("--node", default="node")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--allow-mismatch",
        action="store_true",
        help="continue diagnostic sweeps whose temporary patch breaks coherency",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    workloads = {
        "binary-dataview-set": args.set_app.resolve(),
        "binary-dataview-get": args.get_app.resolve(),
    }
    env = dict(os.environ, TZ="UTC", LC_ALL="en_US.UTF-8")
    rows = []
    points = {}
    for name, app in workloads.items():
        points[name] = {"node": [], "perry": []}
        stopped = set()
        for n in SIZES:
            pair = {}
            commands = {
                "node": [args.node, str(root / f"{name}.ts")],
                "perry": [str(app)],
            }
            for engine, command in commands.items():
                if engine in stopped:
                    continue
                try:
                    proc = subprocess.run(
                        command + [str(n)],
                        capture_output=True,
                        text=True,
                        timeout=60,
                        env=env,
                    )
                    if proc.returncode:
                        row = {
                            "workload": name,
                            "engine": engine,
                            "n": n,
                            "status": "ERROR",
                            "code": proc.returncode,
                            "stderr": proc.stderr,
                            "stdout": proc.stdout,
                        }
                        stopped.add(engine)
                    else:
                        result = json.loads(proc.stderr)
                        row = {"workload": name, "engine": engine, **result}
                        pair[engine] = result
                        points[name][engine].append((n, result["ms_per_run"]))
                except subprocess.TimeoutExpired:
                    row = {
                        "workload": name,
                        "engine": engine,
                        "n": n,
                        "status": "TIMEOUT",
                    }
                    stopped.add(engine)
                rows.append(row)
                print(json.dumps(row), flush=True)
                args.output.write_text(
                    json.dumps(rows, indent=2) + "\n", encoding="utf-8"
                )
            if len(pair) == 2 and pair["node"]["checksum"] != pair["perry"]["checksum"]:
                message = f"Checksum mismatch for {name} at n={n}: {pair}"
                if not args.allow_mismatch:
                    raise SystemExit(message)
                print(message, flush=True)
    print(
        "slopes",
        {
            name: {engine: slope(values) for engine, values in engines.items()}
            for name, engines in points.items()
        },
        flush=True,
    )
    if any("status" in row for row in rows):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
