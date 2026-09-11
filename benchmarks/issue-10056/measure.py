"""Run the unchanged #10056 workload serially against Node and a compiled app."""
import argparse
import json
import math
import os
from pathlib import Path
import subprocess


def slope(points):
    if len(points) < 2:
        return None
    x = [math.log(n) for n, ms in points]
    y = [math.log(ms) for n, ms in points]
    mx, my = sum(x) / len(x), sum(y) / len(y)
    return sum((a - mx) * (b - my) for a, b in zip(x, y)) / sum(
        (a - mx) ** 2 for a in x
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path, help="Compiled subarray.ts executable")
    parser.add_argument("--node", default="node")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    commands = {
        "node": [args.node, str(Path(__file__).with_name("subarray.ts"))],
        "perry": [str(args.app.resolve())],
    }
    env = dict(os.environ, TZ="UTC", LC_ALL="en_US.UTF-8")
    stopped, rows = set(), []
    points = {engine: [] for engine in commands}
    for n in [100, 1000, 10000, 100000, 1000000]:
        pair = {}
        for engine, command in commands.items():
            if engine in stopped:
                continue
            try:
                proc = subprocess.run(
                    command + [str(n)], capture_output=True, text=True,
                    timeout=60, env=env,
                )
                if proc.returncode:
                    row = {"engine": engine, "n": n, "status": "ERROR",
                           "code": proc.returncode, "stderr": proc.stderr}
                    stopped.add(engine)
                else:
                    result = json.loads(proc.stderr)
                    row = {"engine": engine, **result}
                    pair[engine] = result
                    points[engine].append((n, result["ms_per_run"]))
            except subprocess.TimeoutExpired:
                row = {"engine": engine, "n": n, "status": "TIMEOUT"}
                stopped.add(engine)
            rows.append(row)
            print(json.dumps(row), flush=True)
            args.output.write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
        if len(pair) == 2 and pair["node"]["checksum"] != pair["perry"]["checksum"]:
            raise SystemExit(f"Checksum mismatch at n={n}: {pair}")
    print("slopes", {engine: slope(p) for engine, p in points.items()}, flush=True)
    if stopped:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
