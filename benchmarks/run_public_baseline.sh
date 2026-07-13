#!/usr/bin/env bash
# Reproduce the versioned public Perry/Node/Bun evidence at one commit.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

EXPECTED_NODE="${PUBLIC_NODE_VERSION:-v22.23.1}"
EXPECTED_BUN="${PUBLIC_BUN_VERSION:-1.3.14}"
MAX_CPU_ACTIVE="25.0"
QUIET_SECONDS="60"
OUT="$ROOT/.bench-results/public"
FINAL="$ROOT/benchmarks/results/public-node-bun-v1.json"
mkdir -p "$OUT"

fail() { echo "public baseline: $*" >&2; exit 2; }

[[ -z "$(git status --porcelain)" ]] || fail "working tree must be clean before measurement"
[[ "$(node --version)" == "$EXPECTED_NODE" ]] || fail "expected Node $EXPECTED_NODE, found $(node --version)"
[[ "$(bun --version)" == "$EXPECTED_BUN" ]] || fail "expected Bun $EXPECTED_BUN, found $(bun --version)"

if [[ "$(uname)" == "Darwin" ]]; then
  pmset -g batt | head -1 | grep -q "AC Power" || fail "macOS host must be connected to AC power"
  command -v taskpolicy >/dev/null 2>&1 || fail "macOS public measurements require taskpolicy on PATH"
fi

wait_for_quiet() {
  echo "Waiting for CPU active <= $MAX_CPU_ACTIVE% for $QUIET_SECONDS consecutive seconds..."
  python3 - "$MAX_CPU_ACTIVE" "$QUIET_SECONDS" <<'PY'
import os
import platform
import re
import subprocess
import sys
import time

limit, required = float(sys.argv[1]), int(sys.argv[2])


def cpu_active_percent():
    system = platform.system()
    if system == "Darwin":
        output = subprocess.run(
            ["top", "-l", "2", "-n", "0", "-s", "1"],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        ).stdout
        idle = re.findall(r"([0-9]+(?:[.,][0-9]+)?)% idle", output)
        if not idle:
            raise RuntimeError("could not read macOS CPU idle percentage")
        return 100.0 - float(idle[-1].replace(",", "."))
    if system == "Linux":
        def counters():
            with open("/proc/stat", encoding="utf-8") as handle:
                values = [int(value) for value in handle.readline().split()[1:]]
            return sum(values), values[3] + values[4]
        total_before, idle_before = counters()
        time.sleep(1)
        total_after, idle_after = counters()
        total_delta = total_after - total_before
        return 100.0 * (1.0 - (idle_after - idle_before) / total_delta)
    cores = os.cpu_count() or 1
    return min(100.0, os.getloadavg()[0] * 100.0 / cores)


quiet_since = None
deadline = time.monotonic() + 900
while time.monotonic() < deadline:
    active = cpu_active_percent()
    now = time.monotonic()
    quiet_since = now if active <= limit and quiet_since is None else quiet_since
    if active > limit:
        quiet_since = None
    if quiet_since is not None and now - quiet_since >= required:
        print(f"Quiet host confirmed: cpu_active={active:.1f}%")
        raise SystemExit(0)
    time.sleep(4)
raise SystemExit("host did not become CPU-quiet within 15 minutes; no evidence was published")
PY
}

echo "Building Perry at $(git rev-parse HEAD)..."
cargo build --release -p perry-runtime -p perry-stdlib -p perry
wait_for_quiet

echo "=== suite ==="
./benchmarks/compare.sh --full --runs 5 --json-out "$OUT/suite.json" --warn-only
wait_for_quiet

echo "=== polyglot ==="
PUBLIC_BENCH_JSON_OUT="$OUT/polyglot.json" ./benchmarks/polyglot/run_all.sh 11
wait_for_quiet

echo "=== JSON polyglot ==="
PUBLIC_BENCH_JSON_OUT="$OUT/json-polyglot.json" RUNS=11 ./benchmarks/json_polyglot/run.sh
wait_for_quiet

echo "=== app patterns ==="
PUBLIC_BENCH_JSON_OUT="$OUT/app-patterns.json" ./benchmarks/app-patterns/run.sh
wait_for_quiet

echo "=== honest bench ==="
HONEST_BENCH_ONLY=1,3 \
HONEST_BENCH_WARMUP=5 \
HONEST_BENCH_MEASURED=20 \
  ./benchmarks/honest_bench/run.sh --strict-output
python3 benchmarks/honest_bench/scripts/report.py

python3 benchmarks/public_baseline.py assemble \
  --suite "$OUT/suite.json" \
  --polyglot "$OUT/polyglot.json" \
  --json-polyglot "$OUT/json-polyglot.json" \
  --app-patterns "$OUT/app-patterns.json" \
  --honest-results benchmarks/honest_bench/results/results.json \
  --honest-metadata benchmarks/honest_bench/results/metadata.json \
  --output "$FINAL"
python3 benchmarks/public_baseline.py render --artifact "$FINAL"
python3 benchmarks/public_baseline.py check --artifact "$FINAL"

echo "Public evidence generated: $FINAL"
