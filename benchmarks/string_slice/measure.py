"""Run the unchanged #10061 workloads sequentially against Node and Perry."""
import argparse
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import statistics
import subprocess


def slope(rows):
    pairs = [(math.log(r["n"]), math.log(r["ms_per_run"])) for r in rows
             if "ms_per_run" in r and "status" not in r]
    if len(pairs) < 2:
        return None
    mx = statistics.mean(x for x, _ in pairs)
    my = statistics.mean(y for _, y in pairs)
    return sum((x - mx) * (y - my) for x, y in pairs) / sum((x - mx) ** 2 for x, _ in pairs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--perry", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--skip-compile", action="store_true")
    args = parser.parse_args()
    folder = Path(__file__).resolve().parent
    exe_suffix = ".exe" if os.name == "nt" else ""
    result = {"revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
              "node": subprocess.check_output(["node", "--version"], text=True).strip(),
              "platform": platform.platform(), "processor": platform.processor(), "workloads": {}}
    for variant in ["ascii", "unicode"]:
        name = "string-slice-parse-loop-" + variant
        source = folder / (name + ".ts")
        binary = folder / (name + exe_suffix)
        if not args.skip_compile:
            subprocess.run([args.perry, "compile", str(source), "--no-auto-optimize", "-o", str(binary)], check=True)
        engines = {}
        for n in [100, 1000, 10000, 100000, 1000000]:
            for engine, command in [("node", ["node", str(source)]), ("perry", [str(binary)])]:
                rows = engines.setdefault(engine, [])
                if any(r.get("status") == "TIMEOUT" for r in rows):
                    continue
                try:
                    run = subprocess.run(command + [str(n)], capture_output=True, text=True, timeout=60)
                    if run.returncode:
                        row = {"n": n, "status": "ERROR", "exit": run.returncode,
                               "stdout": run.stdout, "stderr": run.stderr}
                    else:
                        row = json.loads(run.stdout)
                except subprocess.TimeoutExpired:
                    row = {"n": n, "status": "TIMEOUT"}
                if engine == "perry" and "checksum" in row:
                    oracle = next((r for r in engines.get("node", []) if r["n"] == n), {})
                    if "checksum" in oracle:
                        row["checksum_match"] = row["checksum"] == oracle["checksum"]
                        if not row["checksum_match"]:
                            row["status"] = "CHECKSUM_MISMATCH"
                rows.append(row)
                print(variant, engine, row, flush=True)
                result["workloads"][variant] = {"sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                                                  "engines": engines,
                                                  "slopes": {e: slope(rs) for e, rs in engines.items()}}
                Path(args.output).write_text(json.dumps(result, indent=2) + "\n", encoding="utf8")


if __name__ == "__main__":
    main()
