#!/usr/bin/env python3
"""Compile and run #10059's Node/Perry comparison workloads."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import time


WORKLOADS = {
    "object-in": [100, 1_000, 10_000, 100_000, 1_000_000],
    "object-has-own-property": [100, 1_000, 10_000, 100_000, 1_000_000],
    "object-assign": [100, 1_000, 10_000],
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def slope(rows: list[dict]) -> float | None:
    if len(rows) < 2:
        return None
    xs = [math.log(row["n"]) for row in rows]
    ys = [math.log(row["ms_per_run"]) for row in rows]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sum(
        (x - mx) ** 2 for x in xs
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("label")
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--runtime-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--artifact-source", required=True)
    parser.add_argument("--require-checksum-match", action="store_true")
    args = parser.parse_args()

    here = Path(__file__).resolve().parent
    args.output.mkdir(parents=True, exist_ok=True)
    compiler = args.compiler.resolve()
    runtime = args.runtime_dir.resolve()
    env = os.environ | {
        "PERRY_RUNTIME_DIR": str(runtime),
        "PERRY_NO_AUTO_OPTIMIZE": "1",
        "TZ": "UTC",
        "LC_ALL": "en_US.UTF-8",
    }
    result = {
        "label": args.label,
        "artifact_source": args.artifact_source,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "worktree_head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "node": subprocess.check_output(["node", "--version"], text=True).strip(),
        "host": {"platform": platform.platform(), "machine": platform.machine(), "cpu_count": os.cpu_count()},
        "load_start": os.getloadavg(),
        "artifacts": {
            name: sha256(runtime / name)
            for name in ["perry", "libperry_runtime.a", "libperry_stdlib.a"]
        },
        "workloads": {},
    }

    for name, sizes in WORKLOADS.items():
        source = here / f"{name}.ts"
        binary = args.output / name
        compile_run = subprocess.run(
            [str(compiler), "compile", str(source), "--no-auto-optimize", "--no-cache", "-o", str(binary)],
            env=env,
            capture_output=True,
            text=True,
            timeout=180,
        )
        (args.output / f"{name}.compile.log").write_text(compile_run.stdout + compile_run.stderr)
        compile_run.check_returncode()
        rows = []
        for n in sizes:
            pair = {}
            for engine, command in [
                ("node", ["node", "--experimental-strip-types", str(source)]),
                ("perry", [str(binary)]),
            ]:
                run = subprocess.run(
                    command + [str(n)], env=env, capture_output=True, text=True, timeout=60
                )
                if run.returncode:
                    raise RuntimeError(f"{name} {engine} n={n}: {run.stderr}{run.stdout}")
                pair[engine] = json.loads(run.stdout)
            checksum_match = pair["node"]["checksum"] == pair["perry"]["checksum"]
            if args.require_checksum_match and not checksum_match:
                raise RuntimeError(f"checksum mismatch: {name} n={n}: {pair}")
            rows.append({
                "n": n,
                "node": pair["node"],
                "perry": pair["perry"],
                "checksum_match": checksum_match,
                "ratio": pair["perry"]["ms_per_run"] / pair["node"]["ms_per_run"],
            })
            print(name, n, "ratio", rows[-1]["ratio"], "checksum_match", checksum_match, flush=True)
        result["workloads"][name] = {
            "source_sha256": sha256(source),
            "rows": rows,
            "node_slope": slope([row["node"] for row in rows]),
            "perry_slope": slope([row["perry"] for row in rows]),
        }
        args.output.joinpath("results.json").write_text(json.dumps(result, indent=2) + "\n")

    result["load_end"] = os.getloadavg()
    args.output.joinpath("results.json").write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
