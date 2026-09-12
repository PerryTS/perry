"""Sequential checksum-gated issue #10087 benchmark (60 s per process)."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess


WORKLOADS = (
    "array-splice-middle-remove",
    "array-splice-middle-insert",
    "array-unshift-build",
)


def artifact_hashes(perry, node, sources):
    windows = os.name == "nt"
    paths = {"perry": perry, "node": Path(node)}
    for name in ("runtime", "stdlib"):
        paths[name] = perry.parent / (f"perry_{name}.lib" if windows else f"libperry_{name}.a")
    paths.update({source.name: source for source in sources})
    return {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in paths.items()}


def slope(rows):
    if len(rows) < 2:
        return None
    x = [math.log(n) for n, _ in rows]
    y = [math.log(t) for _, t in rows]
    mx, my = sum(x) / len(x), sum(y) / len(y)
    return sum((a - mx) * (b - my) for a, b in zip(x, y)) / sum(
        (a - mx) ** 2 for a in x
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--node", default="node")
    args = parser.parse_args()
    args.node = subprocess.check_output([args.node, "-p", "process.execPath"], text=True).strip()
    root = Path(__file__).resolve().parent
    sources = [root / f"{name}.ts" for name in WORKLOADS]
    env = dict(
        os.environ,
        PERRY_RUNTIME_DIR=str(args.perry.resolve().parent),
        TZ="UTC",
        LC_ALL="en_US.UTF-8",
    )
    result = {
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "node": subprocess.check_output([args.node, "--version"], text=True).strip(),
        "host": platform.platform(),
        "cpu": platform.processor(),
        "artifact_sha256": artifact_hashes(args.perry.resolve(), args.node, sources),
        "workloads": {},
    }
    for name in WORKLOADS:
        source = root / f"{name}.ts"
        binary = args.output.resolve().parent / (name + (".exe" if os.name == "nt" else ""))
        subprocess.run(
            [
                str(args.perry.resolve()),
                "compile",
                str(source),
                "--no-auto-optimize",
                "--no-cache",
                "-o",
                str(binary),
            ],
            env=env,
            check=True,
        )
        rows, stopped = [], set()
        for n in (100, 1000, 10000, 100000):
            pair = {}
            for engine, command in (
                ("node", [args.node, str(source)]),
                ("perry", [str(binary)]),
            ):
                if engine in stopped:
                    row = {"status": "SKIPPED"}
                else:
                    try:
                        process = subprocess.run(
                            command + [str(n)],
                            env=env,
                            capture_output=True,
                            text=True,
                            timeout=60,
                        )
                    except subprocess.TimeoutExpired:
                        stopped.add(engine)
                        row = {"status": "TIMEOUT"}
                    else:
                        if process.returncode:
                            raise RuntimeError(
                                f"{engine} {name} {n}: {process.returncode}\n"
                                f"{process.stdout}\n{process.stderr}"
                            )
                        row = dict(json.loads(process.stdout), status="OK")
                pair[engine] = row
                print(name, n, engine, json.dumps(row), flush=True)
            if all(pair[engine]["status"] == "OK" for engine in ("node", "perry")):
                assert pair["node"]["checksum"] == pair["perry"]["checksum"], pair
            rows.append({"n": n, **pair})
            result["workloads"][name] = {"rows": rows}
            args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        common = [row for row in rows if all(row[e]["status"] == "OK" for e in ("node", "perry"))]
        result["workloads"][name].update(
            common_sizes=[row["n"] for row in common],
            common_slopes={
                engine: slope([(row["n"], row[engine]["ms_per_run"]) for row in common])
                for engine in ("node", "perry")
            },
        )
        acceptance = [row for row in common if 1000 <= row["n"] <= 100000]
        acceptance_slopes = {
            engine: slope([(row["n"], row[engine]["ms_per_run"]) for row in acceptance])
            for engine in ("node", "perry")
        }
        acceptance_slope_delta = (
            acceptance_slopes["perry"] - acceptance_slopes["node"]
            if all(value is not None for value in acceptance_slopes.values())
            else None
        )
        result["workloads"][name].update(
            acceptance_sizes=[row["n"] for row in acceptance],
            acceptance_slopes=acceptance_slopes,
            acceptance_slope_delta=acceptance_slope_delta,
        )
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
