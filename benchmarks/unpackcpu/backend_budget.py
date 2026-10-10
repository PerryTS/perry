"""Linux instruction witness for the preserved 22-tarball unpack fixture."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary", type=Path)
parser.add_argument("driver", type=Path)
parser.add_argument("--node", default="node")
parser.add_argument("--output", type=Path, help="Save the counters, digests and result under this prefix")
args = parser.parse_args()
flags = set(Path("/proc/cpuinfo").read_text().split("flags", 1)[1].split("\n", 1)[0].split())
assert {"avx2", "bmi1", "bmi2"} <= flags, "Requires an AVX2/BMI1/BMI2 CPU"
with tempfile.TemporaryDirectory(prefix="perry-wide-digest-") as directory:
    counters = Path(directory) / "counters"
    run = subprocess.run([
        "taskset", "-c", "0-55", "setarch", "-R", "perf", "stat", "-x", ",",
        "-e", "instructions:u", "-o", str(counters),
        str(args.binary.resolve()), "hash", "all", "16",
    ], capture_output=True, timeout=90)
    oracle = subprocess.run([
        args.node, "--experimental-strip-types", str(args.driver.resolve()), "hash", "all", "16",
    ], capture_output=True, timeout=90)
    assert run.returncode == oracle.returncode == 0 and run.stdout == oracle.stdout, "Digest parity failed"
    instructions = next(int(line.split(",")[0]) for line in counters.read_text().splitlines()
                        if ",instructions:u," in line)
    budget = 6_500_000_000
    row = {"binary": str(args.binary.resolve()), "instructions": instructions,
           "budget": budget, "parity": True, "pass_budget": instructions <= budget}
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        Path(str(args.output) + '.json').write_text(json.dumps(row, indent=2) + '\n')
        Path(str(args.output) + '.perf').write_text(counters.read_text())
        Path(str(args.output) + '.out').write_bytes(run.stdout)
        Path(str(args.output) + '.node.out').write_bytes(oracle.stdout)
    print(json.dumps(row))
    assert instructions <= budget, "SHA-512 backend exceeded the instruction budget"
