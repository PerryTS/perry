#!/usr/bin/env bash
# Measure the GC ratchet on a quiet host: either re-pin the baseline, or check
# against it with the memory and timing metrics gated.
#
# THIS IS NOT benchmarks/run_public_baseline.sh. That script produces the
# maintainer-owned public Node/Bun evidence in
# benchmarks/results/public-node-bun-v1.json and gates `lint`. This one produces
# the internal Perry-vs-Perry GC ratchet in
# benchmarks/gc_ratchet/baseline/gc-ratchet-v1.json and gates the `gc-ratchet`
# CI job. The quiet-host discipline below is copied from that script on purpose;
# the artifacts are unrelated and must never be regenerated from one another.
#
# Usage:
#   PERRY_BIN=/path/to/target/release/perry \
#   PERRY_RUNTIME_DIR=/path/to/target/release \
#     ./benchmarks/gc_ratchet/run_gc_ratchet_baseline.sh --check
#
#   ... --pin --notes "why this shift is intentional"
#
# --check  measure and compare against the pinned artifact under the
#          `pinned_host` profile, which gates RSS and wall time as well as
#          retention and the evacuation counters. Default.
# --pin    overwrite the pinned artifact. Do this ONLY when a shift has been
#          reviewed and accepted. Re-pinning to make a red gate go green defeats
#          the entire point of the ratchet.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/benchmarks/gc_ratchet"
cd "$ROOT"

MAX_CPU_ACTIVE="${GC_RATCHET_MAX_CPU_ACTIVE:-25.0}"
QUIET_SECONDS="${GC_RATCHET_QUIET_SECONDS:-60}"
REPEATS="${GC_RATCHET_REPEATS:-7}"
MIN_FREE_GB="${GC_RATCHET_MIN_FREE_GB:-9}"
OUT="$ROOT/.bench-results/gc-ratchet"
ARTIFACT="$HERE/baseline/gc-ratchet-v1.json"
MODE="check"
NOTES=""
RUN_SUITE=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --check) MODE="check"; shift ;;
    --pin) MODE="pin"; shift ;;
    --notes) NOTES="$2"; shift 2 ;;
    --no-suite) RUN_SUITE=0; shift ;;
    --out) OUT="$2"; shift 2 ;;
    --artifact) ARTIFACT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$OUT"
fail() { echo "gc-ratchet: $*" >&2; exit 2; }

# ---------------------------------------------------------------------------
# Preconditions. Every one of these has cost this project a wrong conclusion.
# ---------------------------------------------------------------------------

# A dirty tree means the recorded commit does not describe what was measured.
[[ -z "$(git status --porcelain)" ]] || fail "working tree must be clean before measurement"

# Build outputs are invisible to `git status`, so the binary under test has to
# be named explicitly. An implicit search can silently pick up an archive from
# another worktree that has nothing to do with this commit.
[[ -n "${PERRY_BIN:-}" ]] || fail "set PERRY_BIN to the perry binary under test"
[[ -x "${PERRY_BIN}" ]] || fail "PERRY_BIN=$PERRY_BIN is not executable"
[[ -n "${PERRY_RUNTIME_DIR:-}" ]] || fail "set PERRY_RUNTIME_DIR to the directory holding libperry_{runtime,stdlib}.a"
for lib in libperry_runtime.a libperry_stdlib.a; do
  [[ -f "$PERRY_RUNTIME_DIR/$lib" ]] || fail \
    "$PERRY_RUNTIME_DIR/$lib is missing. perry-runtime and perry-stdlib are rlib-only; the archives come from the perry-runtime-static / perry-stdlib-static wrapper crates. Build: cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static"
done
export PERRY_RUNTIME_DIR
# Deterministic link: use the prebuilt full stdlib rather than letting the
# auto-optimizer rebuild a workload-specific archive mid-measurement.
export PERRY_NO_AUTO_OPTIMIZE=1

# Measuring on a nearly full volume distorts everything downstream.
free_gb=$(df -g "$ROOT" | awk 'NR==2 {print $4}')
[[ "${free_gb:-0}" -ge "$MIN_FREE_GB" ]] || fail "only ${free_gb}GB free; refusing to measure below ${MIN_FREE_GB}GB"

if [[ "$(uname)" == "Darwin" ]]; then
  pmset -g batt | head -1 | grep -q "AC Power" || fail "macOS host must be connected to AC power"
fi

# The Node oracle is a correctness input, not a toolchain detail: a probe that
# quietly stops allocating still exits 0 and reports a tiny retained heap.
EXPECTED_NODE="v$(tr -d 'v \n' < "$ROOT/.node-version")"
uname_s="$(uname -s | tr '[:upper:]' '[:lower:]')"
NODE_BIN="${GC_RATCHET_NODE:-$HOME/node-${EXPECTED_NODE}-${uname_s}-$(uname -m)/bin/node}"
[[ -x "$NODE_BIN" ]] || fail "pinned Node oracle not found at $NODE_BIN (expected $EXPECTED_NODE); set GC_RATCHET_NODE"
actual_node="$("$NODE_BIN" --version)"
[[ "$actual_node" == "$EXPECTED_NODE" ]] || fail "expected Node $EXPECTED_NODE, found $actual_node at $NODE_BIN"

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
            capture_output=True, text=True, timeout=20, check=True,
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
        return 100.0 * (1.0 - (idle_after - idle_before) / (total_after - total_before))
    cores = os.cpu_count() or 1
    return min(100.0, os.getloadavg()[0] * 100.0 / cores)


quiet_since = None
deadline = time.monotonic() + 900
while time.monotonic() < deadline:
    active = cpu_active_percent()
    now = time.monotonic()
    if active > limit:
        quiet_since = None
    elif quiet_since is None:
        quiet_since = now
    if quiet_since is not None and now - quiet_since >= required:
        print(f"Quiet host confirmed: cpu_active={active:.1f}%")
        raise SystemExit(0)
    time.sleep(4)
raise SystemExit("host did not become CPU-quiet within 15 minutes; nothing was written")
PY
}

echo "gc-ratchet ($MODE) at $(git rev-parse HEAD)"
echo "  perry:   $PERRY_BIN"
echo "  runtime: $PERRY_RUNTIME_DIR"
echo "  node:    $NODE_BIN ($actual_node)"
uptime

wait_for_quiet

echo "=== retention + GC counters (${REPEATS} repeats) ==="
python3 "$HERE/gc_ratchet.py" measure \
  --perry "$PERRY_BIN" \
  --repeats "$REPEATS" \
  --node "$NODE_BIN" \
  --output "$OUT/measurement.json"

if [[ "$MODE" == "check" ]]; then
  python3 "$HERE/gc_ratchet.py" check \
    --artifact "$ARTIFACT" \
    --current "$OUT/measurement.json" \
    --profile pinned_host
  exit $?
fi

SUITE_ARG=()
if [[ $RUN_SUITE -eq 1 ]]; then
  wait_for_quiet
  echo "=== benchmark suite (wall + RSS, recorded for the record) ==="
  PATH="$(dirname "$NODE_BIN"):$PATH" PERRY_BIN="$PERRY_BIN" \
    "$ROOT/benchmarks/compare.sh" --full --runs 5 --json-out "$OUT/suite.json" --warn-only
  SUITE_ARG=(--suite "$OUT/suite.json")
fi

python3 "$HERE/gc_ratchet.py" assemble \
  --measurement "$OUT/measurement.json" \
  --tolerances "$HERE/tolerances.json" \
  --perry "$PERRY_BIN" \
  --commit "$(git rev-parse HEAD)" \
  "${SUITE_ARG[@]}" \
  --notes "$NOTES" \
  --output "$ARTIFACT"

python3 "$HERE/gc_ratchet.py" validate --artifact "$ARTIFACT"
echo "gc-ratchet baseline written: $ARTIFACT"
