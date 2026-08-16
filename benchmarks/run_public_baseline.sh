#!/usr/bin/env bash
# Reproduce the versioned public Perry/Node/Bun evidence at one commit.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MEASUREMENT_CONFIG="$ROOT/benchmarks/public-baseline-config.json"
IFS=$'\t' read -r \
  CONFIG_NODE CONFIG_BUN MAX_CPU_ACTIVE QUIET_SECONDS \
  SUITE_RUNS POLYGLOT_RUNS JSON_POLYGLOT_RUNS \
  APP_WARMUP APP_RUNS HONEST_WORKLOADS HONEST_WARMUP HONEST_RUNS < <(
    python3 - "$MEASUREMENT_CONFIG" <<'PY'
import sys
from pathlib import Path

from benchmarks.public_baseline import load_measurement_config

config = load_measurement_config(Path(sys.argv[1]))
components = config["components"]
print(
    config["toolchains"]["node"],
    config["toolchains"]["bun"],
    config["quiet_host"]["maximum_cpu_active_percent"],
    config["quiet_host"]["consecutive_seconds"],
    components["suite"]["measured_runs"],
    components["polyglot"]["measured_runs"],
    components["json_polyglot"]["measured_runs"],
    components["app_patterns"]["warmup_runs"],
    components["app_patterns"]["measured_runs"],
    ",".join(str(value) for value in components["honest_bench"]["workloads"]),
    components["honest_bench"]["warmup_runs"],
    components["honest_bench"]["measured_runs"],
    sep="\t",
)
PY
  )
EXPECTED_NODE="$CONFIG_NODE"
EXPECTED_BUN="$CONFIG_BUN"
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
# The `-static` wrappers are NOT optional (#7012). `perry-runtime` and
# `perry-stdlib` are `crate-type = ["rlib"]`, so building them alone leaves
# `target/release/` with the `perry` binary and no `libperry_runtime.a` /
# `libperry_stdlib.a` -- every `perry compile` in the measurement legs below
# then dies with "Could not find libperry_runtime.a".
#
# This is invisible on a long-lived working copy, which already has the
# archives from unrelated builds; it only bites a clean checkout, which is
# exactly the situation someone regenerating the published artifact is in.
cargo build --release \
    -p perry-runtime -p perry-stdlib -p perry \
    -p perry-runtime-static -p perry-stdlib-static

for archive in libperry_runtime.a libperry_stdlib.a; do
    [[ -f "target/release/$archive" ]] \
        || fail "build did not produce target/release/$archive -- the -static wrapper crates are required (#7012)"
done
wait_for_quiet

echo "=== suite ==="
./benchmarks/compare.sh --full --runs "$SUITE_RUNS" --json-out "$OUT/suite.json" --warn-only
wait_for_quiet

echo "=== polyglot ==="
PUBLIC_BENCH_JSON_OUT="$OUT/polyglot.json" ./benchmarks/polyglot/run_all.sh "$POLYGLOT_RUNS"
wait_for_quiet

echo "=== JSON polyglot ==="
PUBLIC_BENCH_JSON_OUT="$OUT/json-polyglot.json" RUNS="$JSON_POLYGLOT_RUNS" \
  ./benchmarks/json_polyglot/run.sh
wait_for_quiet

echo "=== app patterns ==="
PUBLIC_BENCH_JSON_OUT="$OUT/app-patterns.json" \
PUBLIC_BENCH_WARMUP="$APP_WARMUP" \
PUBLIC_BENCH_RUNS="$APP_RUNS" \
  ./benchmarks/app-patterns/run.sh
wait_for_quiet

echo "=== honest bench ==="
HONEST_BENCH_ONLY="$HONEST_WORKLOADS" \
HONEST_BENCH_WARMUP="$HONEST_WARMUP" \
HONEST_BENCH_MEASURED="$HONEST_RUNS" \
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
