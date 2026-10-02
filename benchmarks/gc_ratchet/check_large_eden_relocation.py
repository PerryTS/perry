"""Exercise probe 13's relocation path separately from its policy fingerprint."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import tempfile

if __package__:
    from . import gc_ratchet as ratchet
else:
    import gc_ratchet as ratchet


SOURCE = Path(__file__).resolve().parent / "probes/13_large_eden_survivors.ts"


def check_relocation(binary: Path, node: Path, source: Path = SOURCE) -> dict:
    """Keep both raw runs, including failures; promotion alone is not movement."""
    run_env = ratchet.probe_run_env(source)
    if run_env.get("PERRY_GC_SCAVENGE_NURSERY_MB") != "64":
        raise ratchet.RatchetError("probe 13 must retain its 64 MB nursery setting")
    run_env.update(PERRY_GC_PROMOTE_IN_PLACE="0", PERRY_GC_DIAG="1")
    oracle_version = "v" + (ratchet.REPO_ROOT / ".node-version").read_text().strip().lstrip("v")
    result = {
        "probe": source.stem,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "run_env": run_env,
        "runs": [],
        "failures": [],
    }
    for index in range(2):
        run = ratchet.run_once([str(binary)], extra_env=run_env)
        counters = ratchet.parse_gc_diag(run["stderr"])
        correctness = ratchet._check_against_node(node, source, run["stdout"])
        result["runs"].append({**run, "counters": counters, "correctness": correctness})
        if run["returncode"] != 0:
            result["failures"].append(f"run {index + 1}: exited {run['returncode']}")
        if correctness.get("status") != "pass":
            result["failures"].append(f"run {index + 1}: Node parity was not verified")
        if correctness.get("oracle_version") != oracle_version:
            result["failures"].append(f"run {index + 1}: expected Node {oracle_version}")
        # These are positive on actual evacuation into survivor space, whereas
        # promoted_objects also counts survivors whose blocks stayed in place.
        for metric in ("minor_cycles", "copied_objects", "copied_bytes"):
            if counters[metric] <= 0:
                result["failures"].append(f"run {index + 1}: {metric} must be positive")
    result["status"] = "fail" if result["failures"] else "pass"
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", required=True, type=Path)
    parser.add_argument("--node", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    with tempfile.TemporaryDirectory(prefix="gc-ratchet-relocation-") as tmp:
        binary = ratchet.compile_probe(args.perry.resolve(), SOURCE, Path(tmp))
        result = check_relocation(binary, args.node.resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"probe 13 relocation: {result['status']}")
    for failure in result["failures"]:
        print(failure)
    return int(result["status"] != "pass")


if __name__ == "__main__":
    raise SystemExit(main())
