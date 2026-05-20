#!/usr/bin/env bash
# Perry Performance Regression Detector
#
# Runs benchmarks, captures speed (wall_ms) and memory (peak RSS),
# compares against baseline.json, reports regressions.
#
# Usage:
#   ./benchmarks/compare.sh                    # Run + compare against baseline
#   ./benchmarks/compare.sh --update-baseline  # Run + update baseline.json
#   ./benchmarks/compare.sh --quick            # Run only 5 fast benchmarks

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SUITE_DIR="$SCRIPT_DIR/suite"
COMPILETS="$ROOT/target/release/perry"
BASELINE="$SCRIPT_DIR/baseline.json"
VERIFY_OUTPUT="$SCRIPT_DIR/verify_benchmark_output.py"

# Thresholds
SPEED_THRESHOLD=15    # >15% slower = regression
MEMORY_THRESHOLD=25   # >25% more RAM = regression

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

UPDATE_BASELINE=0
QUICK_MODE=0
FULL_MODE=0
RUNS=1
JSON_OUT=""
WARN_ONLY=0
COMPARE_EXIT=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --update-baseline) UPDATE_BASELINE=1; shift ;;
    --quick) QUICK_MODE=1; shift ;;
    --full) FULL_MODE=1; shift ;;
    --runs) RUNS="$2"; shift 2 ;;
    --json-out) JSON_OUT="$2"; shift 2 ;;
    --warn-only) WARN_ONLY=1; shift ;;
    --speed-threshold) SPEED_THRESHOLD="$2"; shift 2 ;;
    --memory-threshold) MEMORY_THRESHOLD="$2"; shift 2 ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ ! -f "$COMPILETS" ]]; then
  echo -e "${RED}Perry not found at $COMPILETS${NC}"
  echo "Run: cargo build --release"
  exit 1
fi

if [[ ! -f "$VERIFY_OUTPUT" ]]; then
  echo -e "${RED}Benchmark output verifier not found at $VERIFY_OUTPUT${NC}"
  exit 1
fi

# Select benchmarks
if [[ $QUICK_MODE -eq 1 ]]; then
  BENCHMARKS="02_loop_overhead.ts 05_fibonacci.ts 06_math_intensive.ts 10_nested_loops.ts 13_factorial.ts"
elif [[ $FULL_MODE -eq 1 ]]; then
  # Full suite including the regression-probe benchmarks added for performance tracking
  BENCHMARKS="02_loop_overhead.ts 03_array_write.ts 04_array_read.ts 05_fibonacci.ts 06_math_intensive.ts 07_object_create.ts 08_string_concat.ts 09_method_calls.ts 10_nested_loops.ts 11_prime_sieve.ts 12_binary_trees.ts 13_factorial.ts 14_closure.ts 15_mandelbrot.ts 16_matrix_multiply.ts bench_gc_pressure.ts bench_json_roundtrip.ts bench_object_property.ts bench_int_arithmetic.ts bench_buffer_readwrite.ts bench_array_grow.ts bench_string_heavy.ts"
else
  BENCHMARKS="02_loop_overhead.ts 03_array_write.ts 04_array_read.ts 05_fibonacci.ts 06_math_intensive.ts 07_object_create.ts 08_string_concat.ts 09_method_calls.ts 10_nested_loops.ts 11_prime_sieve.ts 12_binary_trees.ts 13_factorial.ts 14_closure.ts 15_mandelbrot.ts 16_matrix_multiply.ts"
fi

# Check for node
HAS_NODE=0
command -v node &>/dev/null && HAS_NODE=1

echo -e "${BOLD}${CYAN}Perry Performance Comparison (speed + RAM)${NC}"
echo ""

# ---------------------------------------------------------------------------
# Run benchmarks and collect results
# ---------------------------------------------------------------------------
RESULTS_FILE=$(mktemp)
RUN_OUTPUT_DIR=$(mktemp -d)

extract_time() {
  echo "$1" | grep -E "^[a-z_]+:[0-9]+" | head -1 | cut -d: -f2
}

measure_rss() {
  # macOS: /usr/bin/time -l reports "peak memory footprint" in bytes on stderr
  # Linux: /usr/bin/time -v reports "Maximum resident set size" in KB on stderr
  local stdout_file="$1"
  local binary="$2"
  shift 2
  local tmp_err=$(mktemp)

  /usr/bin/time -l "$binary" "$@" >"$stdout_file" 2>"$tmp_err"

  local rss_bytes=0
  # macOS newer: "peak memory footprint" in bytes
  local pmf
  pmf=$(grep 'peak memory footprint' "$tmp_err" 2>/dev/null | awk '{print $1}' || true)
  if [[ -n "$pmf" && "$pmf" != "0" ]]; then
    rss_bytes=$pmf
  else
    # macOS older / some versions: "maximum resident set size" in bytes
    local mrs
    mrs=$(grep 'maximum resident set size' "$tmp_err" 2>/dev/null | awk '{print $1}' || true)
    [[ -n "$mrs" ]] && rss_bytes=$mrs
  fi
  local rss_kb=$((rss_bytes / 1024))

  rm -f "$tmp_err"

  echo "$rss_kb"
}

echo -e "${BOLD}Compiling benchmarks...${NC}"
cd "$SUITE_DIR"
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  if ! "$COMPILETS" "$bench" -o "$name" 2>/dev/null; then
    echo -e "  ${RED}FAIL${NC} $bench"
  fi
done
echo ""

echo -e "${BOLD}Running benchmarks...${NC}"
if [[ $HAS_NODE -eq 1 ]]; then
  printf "${BOLD}%-20s %10s %10s %10s %10s %10s %10s %10s${NC}\n" \
    "Benchmark" "Perry ms" "Node ms" "Ratio" "Perry KB" "Node KB" "Mem Ratio" "Correct"
else
  printf "${BOLD}%-20s %10s %10s %10s %10s${NC}\n" "Benchmark" "Perry ms" "Perry KB" "Mem KB" "Correct"
fi
echo "────────────────────────────────────────────────────────────────────────────────────────────"

median() {
  # Median of space-separated integers (simple, small N)
  python3 -c "import sys; xs=sorted(int(x) for x in sys.argv[1:]); print(xs[len(xs)//2] if xs else 0)" "$@"
}

write_unchecked_correctness() {
  local output_file="$1"
  local reference="$2"
  local reason="$3"
  python3 - "$output_file" "$reference" "$reason" <<'PY'
import json
import sys

output_file, reference, reason = sys.argv[1], sys.argv[2], sys.argv[3]
with open(output_file, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "unchecked",
        "reference": reference,
        "actual_lines": [],
        "expected_lines": [],
        "reason": reason,
    }, handle, indent=2)
    handle.write("\n")
PY
}

set +e  # Disable errexit for measurement loop (grep/awk may return non-zero)
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  display=$(echo "$name" | sed 's/^[0-9]*_//')

  # Run Perry RUNS times, take median for stability on CI
  perry_ms="ERR"
  perry_rss=0
  p_out_samples=()
  if [[ -f "$SUITE_DIR/$name" ]]; then
    p_ms_samples=()
    p_rss_samples=()
    for (( run=0; run<RUNS; run++ )); do
      p_out="$RUN_OUTPUT_DIR/$name.perry.$run.out"
      p_out_samples+=("$p_out")
      r_rss=$(measure_rss "$p_out" "$SUITE_DIR/$name")
      r_out=$(cat "$p_out")
      r_ms=$(extract_time "$r_out")
      [[ -n "$r_ms" ]] && p_ms_samples+=("$r_ms")
      [[ "$r_rss" -gt 0 ]] 2>/dev/null && p_rss_samples+=("$r_rss")
    done
    if [[ ${#p_ms_samples[@]} -gt 0 ]]; then
      perry_ms=$(median "${p_ms_samples[@]}")
    fi
    if [[ ${#p_rss_samples[@]} -gt 0 ]]; then
      perry_rss=$(median "${p_rss_samples[@]}")
    fi
  fi

  # Run Node RUNS times, take median
  node_ms="-"
  node_rss=0
  n_out_samples=()
  if [[ $HAS_NODE -eq 1 ]]; then
    n_ms_samples=()
    n_rss_samples=()
    for (( run=0; run<RUNS; run++ )); do
      n_out="$RUN_OUTPUT_DIR/$name.node.$run.out"
      n_out_samples+=("$n_out")
      r_rss=$(measure_rss "$n_out" node "$SUITE_DIR/$bench")
      r_out=$(cat "$n_out")
      r_ms=$(extract_time "$r_out")
      [[ -n "$r_ms" ]] && n_ms_samples+=("$r_ms")
      [[ "$r_rss" -gt 0 ]] 2>/dev/null && n_rss_samples+=("$r_rss")
    done
    if [[ ${#n_ms_samples[@]} -gt 0 ]]; then
      node_ms=$(median "${n_ms_samples[@]}")
    fi
    if [[ ${#n_rss_samples[@]} -gt 0 ]]; then
      node_rss=$(median "${n_rss_samples[@]}")
    fi
  fi

  # Calculate ratios
  speed_ratio="-"
  mem_ratio="-"
  if [[ "$perry_ms" != "ERR" && "$node_ms" != "-" ]]; then
    if [[ "$node_ms" -gt 0 ]] 2>/dev/null; then
      speed_ratio=$(python3 -c "print(f'{int(\"$perry_ms\")/int(\"$node_ms\"):.2f}')" 2>/dev/null || echo "-")
    fi
  fi
  if [[ "$perry_rss" -gt 0 && "$node_rss" -gt 0 ]] 2>/dev/null; then
    mem_ratio=$(python3 -c "print(f'{int(\"$perry_rss\")/int(\"$node_rss\"):.2f}')" 2>/dev/null || echo "-")
  fi

  correctness_json="$RUN_OUTPUT_DIR/$name.correctness.json"
  if [[ $HAS_NODE -ne 1 ]]; then
    write_unchecked_correctness "$correctness_json" "none" "node unavailable"
  elif [[ ${#n_out_samples[@]} -eq 0 ]]; then
    write_unchecked_correctness "$correctness_json" "none" "node produced no stdout sample"
  else
    if [[ ${#p_out_samples[@]} -eq 0 ]]; then
      missing_out="$RUN_OUTPUT_DIR/$name.perry.missing.out"
      : > "$missing_out"
      p_out_samples=("$missing_out")
    fi
    python3 - "$VERIFY_OUTPUT" "${n_out_samples[0]}" "$correctness_json" "${p_out_samples[@]}" <<'PY'
import importlib.util
import json
import sys

verifier_path, expected_path, output_path, *actual_paths = sys.argv[1:]
spec = importlib.util.spec_from_file_location("benchmark_output_verifier", verifier_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

reports = []
for index, actual_path in enumerate(actual_paths, start=1):
    report = module.compare_stdout_files(
        expected_path=expected_path,
        actual_path=actual_path,
        reference="node",
    )
    report["sample"] = index
    reports.append(report)

if not reports:
    merged = {
        "status": "unchecked",
        "reference": "node",
        "actual_lines": [],
        "expected_lines": [],
        "reason": "perry produced no stdout sample",
    }
else:
    failures = [report for report in reports if report["status"] == "fail"]
    passes = [report for report in reports if report["status"] == "pass"]
    if failures:
        first = failures[0]
        merged = {
            "status": "fail",
            "reference": "node",
            "actual_lines": first["actual_lines"],
            "expected_lines": first["expected_lines"],
            "reason": (
                f"{len(failures)}/{len(reports)} Perry sample(s) failed; "
                f"sample {first['sample']}: {first['reason']}"
            ),
        }
    elif passes:
        first = passes[0]
        merged = {
            "status": "pass",
            "reference": "node",
            "actual_lines": first["actual_lines"],
            "expected_lines": first["expected_lines"],
            "reason": f"all {len(reports)} Perry sample(s) matched Node semantic output",
        }
    else:
        first = reports[0]
        merged = {
            "status": "unchecked",
            "reference": "node",
            "actual_lines": first["actual_lines"],
            "expected_lines": first["expected_lines"],
            "reason": first["reason"],
        }

with open(output_path, "w", encoding="utf-8") as handle:
    json.dump(merged, handle, indent=2)
    handle.write("\n")

sys.exit(1 if merged["status"] == "fail" else 0)
PY
  fi
  correctness_status=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['status'])" "$correctness_json")

  if [[ $HAS_NODE -eq 1 ]]; then
    printf "%-20s %10s %10s %10s %10s %10s %10s %10s\n" \
      "$display" "${perry_ms}ms" "${node_ms}ms" "$speed_ratio" "${perry_rss}KB" "${node_rss}KB" "$mem_ratio" "$correctness_status"
  else
    printf "%-20s %10s %10s %10s %10s\n" "$display" "${perry_ms}ms" "${perry_rss}KB" "$mem_ratio" "$correctness_status"
  fi

  # Save result for JSON
  echo "${name}|${perry_ms}|${perry_rss}|${node_ms}|${node_rss}|${correctness_json}" >> "$RESULTS_FILE"
done
set -e

echo ""

# ---------------------------------------------------------------------------
# Generate current results JSON
# ---------------------------------------------------------------------------
if [[ -n "$JSON_OUT" ]]; then
  CURRENT_JSON="$JSON_OUT"
else
  CURRENT_JSON=$(mktemp)
fi
python3 - "$RESULTS_FILE" "$CURRENT_JSON" <<'PYEOF'
import json, sys
results_file, output_file = sys.argv[1], sys.argv[2]
from datetime import datetime, timezone
import subprocess

commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"],
                       capture_output=True, text=True).stdout.strip()

benchmarks = {}
with open(results_file) as f:
    for line in f:
        parts = line.strip().split('|')
        if len(parts) < 6: continue
        name, perry_ms, perry_rss, node_ms, node_rss, correctness_path = parts[:6]
        entry = {
            "perry_ms": int(perry_ms) if perry_ms not in ("ERR", "") else None,
            "perry_rss_kb": int(perry_rss) if perry_rss else 0,
        }
        try:
            with open(correctness_path) as correctness_file:
                entry["correctness"] = json.load(correctness_file)
        except Exception as exc:
            entry["correctness"] = {
                "status": "unchecked",
                "reference": "none",
                "actual_lines": [],
                "expected_lines": [],
                "reason": f"could not read correctness report: {exc}",
            }
        if node_ms not in ("-", ""):
            entry["node_ms"] = int(node_ms)
            entry["node_rss_kb"] = int(node_rss)
            if entry["perry_ms"] and entry["node_ms"]:
                entry["speed_ratio"] = round(entry["perry_ms"] / entry["node_ms"], 3)
            if entry["perry_rss_kb"] and entry["node_rss_kb"]:
                entry["memory_ratio"] = round(entry["perry_rss_kb"] / entry["node_rss_kb"], 3)
        benchmarks[name] = entry

result = {
    "commit": commit,
    "generated_at": datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
    "benchmarks": benchmarks
}
with open(output_file, 'w') as f:
    json.dump(result, f, indent=2)
PYEOF

CORRECTNESS_FAIL_COUNT=$(python3 - "$CURRENT_JSON" <<'PY'
import json
import sys

current = json.load(open(sys.argv[1]))
print(sum(
    1
    for entry in current.get("benchmarks", {}).values()
    if entry.get("correctness", {}).get("status") == "fail"
))
PY
)

# ---------------------------------------------------------------------------
# Compare against baseline
# ---------------------------------------------------------------------------
if [[ -f "$BASELINE" && $UPDATE_BASELINE -eq 0 ]]; then
  echo -e "${BOLD}Comparing against baseline...${NC}"
  echo ""

  set +e
  python3 - "$BASELINE" "$CURRENT_JSON" "$SPEED_THRESHOLD" "$MEMORY_THRESHOLD" <<'PYEOF'
import json, sys

baseline_file, current_file = sys.argv[1], sys.argv[2]
speed_thresh = int(sys.argv[3])
mem_thresh = int(sys.argv[4])

baseline = json.load(open(baseline_file))
current = json.load(open(current_file))

regressions = []
improvements = []
correctness_failures = []

print(f"Baseline commit: {baseline.get('commit', '?')} | Current commit: {current.get('commit', '?')}")
print(f"Speed threshold: {speed_thresh}% | Memory threshold: {mem_thresh}%")
print()
print(f"{'Benchmark':<20s} {'Correct':>10s} {'Speed Delta':>12s} {'RAM Delta':>10s} {'Status':>12s}")
print("-" * 72)

# Noise floors: percentage swings on tiny measurements are unreliable.
# A 7ms jitter on a 9ms benchmark is 78% but means nothing. Require both
# the absolute delta AND percentage to exceed the threshold.
MIN_SPEED_DELTA_MS = 20   # need at least 20ms absolute change to flag
MIN_RAM_DELTA_KB = 2048   # need at least 2MB absolute change to flag

for name, cur in current["benchmarks"].items():
    correctness = cur.get("correctness", {})
    correctness_status = correctness.get("status", "unchecked")
    if correctness_status == "fail":
        reason = correctness.get("reason", "semantic output mismatch")
        expected = correctness.get("expected_lines", [])
        actual = correctness.get("actual_lines", [])
        correctness_failures.append(
            f"{name}: {reason}; expected={expected!r}; actual={actual!r}"
        )
        print(f"{name.replace('_', ' '):<20s} {correctness_status:>10s} {'-':>12s} {'-':>10s} {'INVALID':>12s}")
        continue

    base = baseline.get("benchmarks", {}).get(name)
    if not base:
        print(f"{name:<20s} {correctness_status:>10s} {'NEW':>12s} {'NEW':>10s} {'new':>12s}")
        continue

    # Speed comparison
    speed_status = "ok"
    speed_delta = "-"
    if cur.get("perry_ms") is not None and base.get("perry_ms") is not None and base["perry_ms"] > 0:
        abs_delta = cur["perry_ms"] - base["perry_ms"]
        pct = abs_delta / base["perry_ms"] * 100
        speed_delta = f"{pct:+.1f}%"
        # Flag only if BOTH percentage AND absolute delta exceed threshold
        if pct > speed_thresh and abs(abs_delta) >= MIN_SPEED_DELTA_MS:
            speed_status = "REGRESSION"
            regressions.append(f"{name}: speed +{pct:.1f}% ({base['perry_ms']}ms -> {cur['perry_ms']}ms)")
        elif pct < -speed_thresh and abs(abs_delta) >= MIN_SPEED_DELTA_MS:
            speed_status = "improved"
            improvements.append(f"{name}: speed {pct:.1f}% ({base['perry_ms']}ms -> {cur['perry_ms']}ms)")

    # Memory comparison
    mem_status = "ok"
    mem_delta = "-"
    if cur.get("perry_rss_kb") and base.get("perry_rss_kb") and base["perry_rss_kb"] > 0:
        abs_delta = cur["perry_rss_kb"] - base["perry_rss_kb"]
        pct = abs_delta / base["perry_rss_kb"] * 100
        mem_delta = f"{pct:+.1f}%"
        if pct > mem_thresh and abs(abs_delta) >= MIN_RAM_DELTA_KB:
            mem_status = "REGRESSION"
            regressions.append(f"{name}: RAM +{pct:.1f}% ({base['perry_rss_kb']}KB -> {cur['perry_rss_kb']}KB)")
        elif pct < -mem_thresh and abs(abs_delta) >= MIN_RAM_DELTA_KB:
            mem_status = "improved"
            improvements.append(f"{name}: RAM {pct:.1f}% ({base['perry_rss_kb']}KB -> {cur['perry_rss_kb']}KB)")

    status = "REGRESSION" if "REGRESSION" in (speed_status, mem_status) else \
             "improved" if "improved" in (speed_status, mem_status) else "ok"
    print(f"{name.replace('_', ' '):<20s} {correctness_status:>10s} {speed_delta:>12s} {mem_delta:>10s} {status:>12s}")

print()
if correctness_failures:
    print(f"{len(correctness_failures)} CORRECTNESS FAILURE(S):")
    for failure in correctness_failures:
        print(f"  - {failure}")
    sys.exit(1)
elif regressions:
    print(f"{len(regressions)} REGRESSION(S):")
    for r in regressions:
        print(f"  - {r}")
    sys.exit(1)
elif improvements:
    print(f"{len(improvements)} improvement(s), no regressions")
else:
    print("No significant changes")
PYEOF
  COMPARE_EXIT=$?
  set -e

  if [[ $COMPARE_EXIT -ne 0 && $WARN_ONLY -eq 1 ]]; then
    echo ""
    echo "--warn-only: benchmark gate failed but not failing build"
    COMPARE_EXIT=0
  fi

elif [[ $UPDATE_BASELINE -eq 1 ]]; then
  if [[ "$CORRECTNESS_FAIL_COUNT" -gt 0 ]]; then
    echo -e "${RED}Refusing to update baseline: correctness gate failed.${NC}"
    python3 - "$CURRENT_JSON" <<'PY'
import json
import sys

current = json.load(open(sys.argv[1]))
for name, entry in current.get("benchmarks", {}).items():
    correctness = entry.get("correctness", {})
    if correctness.get("status") == "fail":
        print(f"  - {name}: {correctness.get('reason', 'semantic output mismatch')}")
PY
    COMPARE_EXIT=1
  else
    cp "$CURRENT_JSON" "$BASELINE"
    echo -e "${GREEN}Baseline updated: $BASELINE${NC}"
    echo "Commit: $(python3 -c "import json; print(json.load(open('$BASELINE'))['commit'])")"
  fi
elif [[ "$CORRECTNESS_FAIL_COUNT" -gt 0 ]]; then
  echo -e "${RED}Correctness gate failed.${NC}"
  python3 - "$CURRENT_JSON" <<'PY'
import json
import sys

current = json.load(open(sys.argv[1]))
for name, entry in current.get("benchmarks", {}).items():
    correctness = entry.get("correctness", {})
    if correctness.get("status") == "fail":
        print(f"  - {name}: {correctness.get('reason', 'semantic output mismatch')}")
PY
  COMPARE_EXIT=1
  if [[ $WARN_ONLY -eq 1 ]]; then
    echo ""
    echo "--warn-only: benchmark gate failed but not failing build"
    COMPARE_EXIT=0
  fi
fi

# Cleanup
rm -f "$RESULTS_FILE"
rm -rf "$RUN_OUTPUT_DIR"
# Only remove CURRENT_JSON if it was a tempfile (not user-requested via --json-out)
[[ -z "$JSON_OUT" ]] && rm -f "$CURRENT_JSON"
cd "$SUITE_DIR" && rm -f 01_startup 02_loop_overhead 03_array_write 04_array_read 05_fibonacci \
  06_math_intensive 07_object_create 08_string_concat 09_method_calls 10_nested_loops \
  11_prime_sieve 12_binary_trees 13_factorial 14_closure 15_mandelbrot 16_matrix_multiply \
  bench_gc_pressure bench_json_roundtrip bench_object_property bench_int_arithmetic \
  bench_buffer_readwrite bench_array_grow bench_string_heavy 2>/dev/null

exit ${COMPARE_EXIT:-0}
