"""Sequential checksum-gated issue #10060 benchmark (60 s per process)."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess


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
    # Execute Node itself: a toolchain shim may spawn a child that inherits
    # the pipes and survives termination of the shim at the process timeout.
    args.node = subprocess.check_output([args.node, "-p", "process.execPath"], text=True).strip()
    root = Path(__file__).resolve().parent
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(args.perry.resolve().parent), TZ="UTC",
               LC_ALL="en_US.UTF-8")
    result = {
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "node": subprocess.check_output([args.node, "--version"], text=True).strip(),
        "host": platform.platform(),
        "cpu": platform.processor(),
        "artifact_sha256": artifact_hashes(args.perry.resolve(), args.node,
                                            [root / f"{name}.ts" for name in
                                             ("array-shift-queue", "array-push")]),
        "workloads": {},
    }
    for name in ("array-shift-queue", "array-push"):
        source = root / f"{name}.ts"
        binary = args.output.resolve().parent / (name + (".exe" if os.name == "nt" else ""))
        subprocess.run([str(args.perry.resolve()), "compile", str(source),
                        "--no-auto-optimize", "--no-cache", "-o", str(binary)], env=env, check=True)
        rows, stopped = [], set()
        for n in (100, 1000, 10000, 100000, 1000000):
            pair = {}
            for engine, command in (("node", [args.node, str(source)]),
                                    ("perry", [str(binary)])):
                if engine in stopped:
                    row = {"status": "SKIPPED"}
                else:
                    try:
                        p = subprocess.run(command + [str(n)], env=env, capture_output=True,
                                           text=True, timeout=60)
                    except subprocess.TimeoutExpired:
                        stopped.add(engine)
                        row = {"status": "TIMEOUT"}
                    else:
                        if p.returncode:
                            raise RuntimeError(f"{engine} {name} {n}: {p.returncode}\n{p.stdout}\n{p.stderr}")
                        row = dict(json.loads(p.stdout), status="OK")
                pair[engine] = row
                print(name, n, engine, json.dumps(row), flush=True)
            if all(r["status"] == "OK" for r in pair.values()):
                assert pair["node"]["checksum"] == pair["perry"]["checksum"], pair
            rows.append({"n": n, **pair})
            result["workloads"][name] = {"rows": rows}
            args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        common = [r for r in rows if all(r[e]["status"] == "OK" for e in ("node", "perry"))]
        result["workloads"][name].update(
            common_sizes=[r["n"] for r in common],
            common_slopes={e: slope([(r["n"], r[e]["ms_per_run"]) for r in common])
                           for e in ("node", "perry")},
        )
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
