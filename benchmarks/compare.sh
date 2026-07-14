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
COMPILETS="${PERRY_BIN:-$ROOT/target/release/perry}"
BASELINE="$SCRIPT_DIR/baseline.json"
VERIFY_OUTPUT="$SCRIPT_DIR/verify_benchmark_output.py"
BENCHMARK_GATE="$SCRIPT_DIR/benchmark_gate.py"

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
RUNS=5
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

# Resolve --json-out to an absolute path up front: the script `cd`s into
# $SUITE_DIR during compilation, so a relative path like
# `.bench-results/current.json` would be created (or fail to be created)
# relative to benchmarks/suite/ instead of the caller's cwd. This silently
# broke the CI regression gate for weeks: the JSON write threw
# FileNotFoundError, `| tee` swallowed the non-zero exit, and the workflow's
# "grep REGRESSION" found nothing — every release-mode performance gate
# passed vacuously.
if [[ -n "$JSON_OUT" ]]; then
  mkdir -p "$(dirname "$JSON_OUT")"
  JSON_OUT="$(cd "$(dirname "$JSON_OUT")" && pwd)/$(basename "$JSON_OUT")"
fi

if [[ ! -f "$COMPILETS" ]]; then
  echo -e "${RED}Perry not found at $COMPILETS${NC}"
  echo "Run: cargo build --release"
  exit 1
fi

if [[ ! -f "$VERIFY_OUTPUT" ]]; then
  echo -e "${RED}Benchmark output verifier not found at $VERIFY_OUTPUT${NC}"
  exit 1
fi

if [[ ! -f "$BENCHMARK_GATE" ]]; then
  echo -e "${RED}Benchmark artifact builder not found at $BENCHMARK_GATE${NC}"
  exit 1
fi

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [[ "$RUNS" -lt 2 ]]; then
  echo "--runs must be an integer of at least 2 so dispersion can be calibrated" >&2
  exit 2
fi

# Select benchmarks
if [[ $QUICK_MODE -eq 1 ]]; then
  BENCHMARKS="02_loop_overhead.ts 05_fibonacci.ts 06_math_intensive.ts 10_nested_loops.ts 13_factorial.ts"
elif [[ $FULL_MODE -eq 1 ]]; then
  # Full suite including the regression-probe benchmarks added for performance tracking
  BENCHMARKS="02_loop_overhead.ts 03_array_write.ts 04_array_read.ts 05_fibonacci.ts 06_math_intensive.ts 07_object_create.ts 08_string_concat.ts 09_method_calls.ts 10_nested_loops.ts 11_prime_sieve.ts 12_binary_trees.ts 13_factorial.ts 14_closure.ts 15_mandelbrot.ts 16_matrix_multiply.ts bench_gc_pressure.ts bench_json_roundtrip.ts bench_object_property.ts bench_int_arithmetic.ts bench_buffer_readwrite.ts bench_array_grow.ts bench_string_heavy.ts bench_numeric_array_numeric.ts bench_numeric_array_downgrade.ts"
else
  BENCHMARKS="02_loop_overhead.ts 03_array_write.ts 04_array_read.ts 05_fibonacci.ts 06_math_intensive.ts 07_object_create.ts 08_string_concat.ts 09_method_calls.ts 10_nested_loops.ts 11_prime_sieve.ts 12_binary_trees.ts 13_factorial.ts 14_closure.ts 15_mandelbrot.ts 16_matrix_multiply.ts"
fi
EXPECTED_BENCHMARKS=""
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  EXPECTED_BENCHMARKS+="${EXPECTED_BENCHMARKS:+,}$name"
done

# Check for node
HAS_NODE=0
NODE_CMD=(node)

detect_node_ts_runner() {
  command -v node &>/dev/null || return 1

  local probe
  probe=$(mktemp "${TMPDIR:-/tmp}/perry-node-ts-probe.XXXXXX.ts")
  printf 'const x: number = 1;\nconsole.log("node_ts_probe:" + x);\n' >"$probe"

  if node "$probe" >/dev/null 2>&1; then
    NODE_CMD=(node)
    rm -f "$probe"
    return 0
  fi

  if node --experimental-strip-types "$probe" >/dev/null 2>&1; then
    NODE_CMD=(node --experimental-strip-types)
    rm -f "$probe"
    return 0
  fi

  rm -f "$probe"
  return 1
}

if detect_node_ts_runner; then
  HAS_NODE=1
else
  echo "Node.js is unavailable for .ts benchmark inputs; Node columns and correctness checks will be skipped." >&2
fi

# Bun runs TypeScript directly. It is optional for local runs, but CI installs
# an exact version and the artifact always records whether it was available.
HAS_BUN=0
BUN_CMD=(bun run)
if command -v bun &>/dev/null; then
  HAS_BUN=1
else
  echo "Bun is unavailable; Bun distributions and ratios will be marked unavailable." >&2
fi

RUNTIME_METADATA=$(mktemp)
trap 'rm -f "$RUNTIME_METADATA"' EXIT
python3 - "$RUNTIME_METADATA" "$COMPILETS" "$HAS_NODE" "$HAS_BUN" \
  "$("$COMPILETS" --version 2>/dev/null || echo local-build)" \
  "$(node --version 2>/dev/null || true)" "$(bun --version 2>/dev/null || true)" \
  "${NODE_CMD[*]}" <<'PY'
import json
import sys

path, perry, has_node, has_bun, perry_version, node_version, bun_version, node_command = sys.argv[1:]
metadata = {
    "perry": {
        "available": True,
        "version": perry_version.strip() or "local-build",
        "command": ["<compiled-binary>"],
        "compile_command": [perry, "<source.ts>", "-o", "<compiled-binary>"],
    },
    "node": {
        "available": has_node == "1",
        "version": node_version.strip() or None,
        "command": node_command.split() + ["<source.ts>"],
    },
    "bun": {
        "available": has_bun == "1",
        "version": bun_version.strip() or None,
        "command": ["bun", "run", "<source.ts>"],
    },
}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(metadata, handle, indent=2)
    handle.write("\n")
PY

echo -e "${BOLD}${CYAN}Perry Performance Comparison (speed + RAM)${NC}"
echo ""

# ---------------------------------------------------------------------------
# Run benchmarks and collect results
# ---------------------------------------------------------------------------
RESULTS_FILE=$(mktemp)
RUN_OUTPUT_DIR=$(mktemp -d)
CURRENT_JSON=""

cleanup() {
  rm -f "$RESULTS_FILE" "$RUNTIME_METADATA"
  rm -rf "$RUN_OUTPUT_DIR"
  if [[ -n "$CURRENT_JSON" && -z "$JSON_OUT" ]]; then
    rm -f "$CURRENT_JSON"
  fi
  for bench in $BENCHMARKS; do
    rm -f "$SUITE_DIR/${bench%.ts}"
  done
}
trap cleanup EXIT

extract_time() {
  awk -F: '/^[a-z_]+:[0-9]+/ {print $2; exit}' <<<"$1"
}

measure_rss() {
  # macOS: /usr/bin/time -l reports peak RSS in bytes.
  # Linux: /usr/bin/time -v reports peak RSS in KB.
  local stdout_file="$1"
  local binary="$2"
  shift 2
  local tmp_err=$(mktemp)
  local command_status=0

  if [[ "$(uname)" == "Darwin" ]]; then
    # `time -l` calls sysctl(kern.clockrate), which fails in otherwise usable
    # sandboxed macOS environments and masks a successful benchmark exit.
    # A fresh Python process can obtain the same child peak RSS from wait4.
    local measurement
    measurement=$(python3 - "$stdout_file" "$tmp_err" "$binary" "$@" <<'PYTHON'
import resource
import subprocess
import sys

stdout_path, stderr_path, *command = sys.argv[1:]
with open(stdout_path, "wb") as stdout, open(stderr_path, "wb") as stderr:
    completed = subprocess.run(command, stdout=stdout, stderr=stderr, check=False)
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
# Darwin reports ru_maxrss in bytes; the artifact schema records KiB.
print(f"{int(usage.ru_maxrss) // 1024}|{completed.returncode}")
PYTHON
    )
    rss_kb="${measurement%%|*}"
    command_status="${measurement##*|}"
  elif [[ -x /usr/bin/time ]]; then
    /usr/bin/time -v "$binary" "$@" >"$stdout_file" 2>"$tmp_err" || command_status=$?
  else
    "$binary" "$@" >"$stdout_file" 2>"$tmp_err" || command_status=$?
  fi

  local rss_kb=${rss_kb:-0}
  if [[ "$(uname)" != "Darwin" ]]; then
    local linux_kb
    linux_kb=$(awk -F': ' '/Maximum resident set size/ {print $2; exit}' "$tmp_err" 2>/dev/null || true)
    linux_kb=${linux_kb:-0}
    rss_kb=$linux_kb
  fi

  rm -f "$tmp_err"

  echo "$rss_kb|$command_status"
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
printf "${BOLD}%-20s %10s %10s %10s %10s %10s %10s %10s${NC}\n" \
  "Benchmark" "Perry ms" "Node ms" "Bun ms" "P/Node" "P/Bun" "Perry KB" "Correct"
echo "────────────────────────────────────────────────────────────────────────────────────────────"

median() {
  # Median of space-separated integers (simple, small N)
  python3 -c "import sys; xs=sorted(int(x) for x in sys.argv[1:]); print(xs[len(xs)//2] if xs else 0)" "$@"
}

join_samples() {
  local IFS=,
  printf '%s' "$*"
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
  p_ms_samples=()
  p_rss_samples=()
  if [[ -f "$SUITE_DIR/$name" ]]; then
    for (( run=0; run<RUNS; run++ )); do
      p_out="$RUN_OUTPUT_DIR/$name.perry.$run.out"
      p_out_samples+=("$p_out")
      measurement=$(measure_rss "$p_out" "$SUITE_DIR/$name")
      r_rss="${measurement%%|*}"
      r_status="${measurement##*|}"
      r_out=$(cat "$p_out")
      r_ms=$(extract_time "$r_out")
      if [[ "$r_status" -eq 0 && -n "$r_ms" ]]; then
        p_ms_samples+=("$r_ms")
        p_rss_samples+=("${r_rss:-0}")
      fi
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
  n_ms_samples=()
  n_rss_samples=()
  if [[ $HAS_NODE -eq 1 ]]; then
    for (( run=0; run<RUNS; run++ )); do
      n_out="$RUN_OUTPUT_DIR/$name.node.$run.out"
      n_out_samples+=("$n_out")
      measurement=$(measure_rss "$n_out" "${NODE_CMD[@]}" "$SUITE_DIR/$bench")
      r_rss="${measurement%%|*}"
      r_status="${measurement##*|}"
      r_out=$(cat "$n_out")
      r_ms=$(extract_time "$r_out")
      if [[ "$r_status" -eq 0 && -n "$r_ms" ]]; then
        n_ms_samples+=("$r_ms")
        n_rss_samples+=("${r_rss:-0}")
      fi
    done
    if [[ ${#n_ms_samples[@]} -gt 0 ]]; then
      node_ms=$(median "${n_ms_samples[@]}")
    fi
    if [[ ${#n_rss_samples[@]} -gt 0 ]]; then
      node_rss=$(median "${n_rss_samples[@]}")
    fi
  fi

  # Run Bun RUNS times, take median. Missing Bun is an explicit supported
  # fallback for local development; CI pins and installs it.
  bun_ms="-"
  bun_rss=0
  b_out_samples=()
  b_ms_samples=()
  b_rss_samples=()
  if [[ $HAS_BUN -eq 1 ]]; then
    for (( run=0; run<RUNS; run++ )); do
      b_out="$RUN_OUTPUT_DIR/$name.bun.$run.out"
      b_out_samples+=("$b_out")
      measurement=$(measure_rss "$b_out" "${BUN_CMD[@]}" "$SUITE_DIR/$bench")
      r_rss="${measurement%%|*}"
      r_status="${measurement##*|}"
      r_out=$(cat "$b_out")
      r_ms=$(extract_time "$r_out")
      if [[ "$r_status" -eq 0 && -n "$r_ms" ]]; then
        b_ms_samples+=("$r_ms")
        b_rss_samples+=("${r_rss:-0}")
      fi
    done
    if [[ ${#b_ms_samples[@]} -gt 0 ]]; then
      bun_ms=$(median "${b_ms_samples[@]}")
    fi
    if [[ ${#b_rss_samples[@]} -gt 0 ]]; then
      bun_rss=$(median "${b_rss_samples[@]}")
    fi
  fi

  # Calculate ratios
  speed_ratio="-"
  bun_speed_ratio="-"
  mem_ratio="-"
  if [[ "$perry_ms" != "ERR" && "$node_ms" != "-" ]]; then
    if [[ "$node_ms" -gt 0 ]] 2>/dev/null; then
      speed_ratio=$(python3 -c "print(f'{int(\"$perry_ms\")/int(\"$node_ms\"):.2f}')" 2>/dev/null || echo "-")
    fi
  fi
  if [[ "$perry_ms" != "ERR" && "$bun_ms" != "-" ]]; then
    if [[ "$bun_ms" -gt 0 ]] 2>/dev/null; then
      bun_speed_ratio=$(python3 -c "print(f'{int(\"$perry_ms\")/int(\"$bun_ms\"):.2f}')" 2>/dev/null || echo "-")
    fi
  fi
  if [[ "$perry_rss" -gt 0 && "$node_rss" -gt 0 ]] 2>/dev/null; then
    mem_ratio=$(python3 -c "print(f'{int(\"$perry_rss\")/int(\"$node_rss\"):.2f}')" 2>/dev/null || echo "-")
  fi

  correctness_json="$RUN_OUTPUT_DIR/$name.correctness.json"
  reference="node"
  reference_outputs=("${n_out_samples[@]}")
  if [[ $HAS_NODE -ne 1 ]]; then
    reference="bun"
    reference_outputs=("${b_out_samples[@]}")
  fi
  if [[ $HAS_NODE -ne 1 && $HAS_BUN -ne 1 ]]; then
    write_unchecked_correctness "$correctness_json" "none" "Node and Bun unavailable"
  elif [[ ${#reference_outputs[@]} -eq 0 ]]; then
    write_unchecked_correctness "$correctness_json" "none" "$reference produced no stdout sample"
  else
    if [[ ${#p_out_samples[@]} -eq 0 ]]; then
      missing_out="$RUN_OUTPUT_DIR/$name.perry.missing.out"
      : > "$missing_out"
      p_out_samples=("$missing_out")
    fi
    python3 - "$VERIFY_OUTPUT" "${reference_outputs[0]}" "$correctness_json" "$reference" "${p_out_samples[@]}" <<'PY'
import importlib.util
import json
import sys

verifier_path, expected_path, output_path, reference, *actual_paths = sys.argv[1:]
spec = importlib.util.spec_from_file_location("benchmark_output_verifier", verifier_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

reports = []
for index, actual_path in enumerate(actual_paths, start=1):
    report = module.compare_stdout_files(
        expected_path=expected_path,
        actual_path=actual_path,
        reference=reference,
    )
    report["sample"] = index
    reports.append(report)

if not reports:
    merged = {
        "status": "unchecked",
        "reference": reference,
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
            "reference": reference,
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
            "reference": reference,
            "actual_lines": first["actual_lines"],
            "expected_lines": first["expected_lines"],
            "reason": f"all {len(reports)} Perry sample(s) matched {reference} semantic output",
        }
    else:
        first = reports[0]
        merged = {
            "status": "unchecked",
            "reference": reference,
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

  printf "%-20s %10s %10s %10s %10s %10s %10s %10s\n" \
    "$display" "${perry_ms}ms" "${node_ms}ms" "${bun_ms}ms" "$speed_ratio" "$bun_speed_ratio" "${perry_rss}KB" "$correctness_status"

  # Save raw samples as JSONL. The artifact builder rejects any available
  # runtime that did not produce exactly RUNS timing and RSS samples.
  p_ms_csv=""; p_rss_csv=""; n_ms_csv=""; n_rss_csv=""; b_ms_csv=""; b_rss_csv=""
  [[ ${#p_ms_samples[@]} -gt 0 ]] && p_ms_csv=$(join_samples "${p_ms_samples[@]}")
  [[ ${#p_rss_samples[@]} -gt 0 ]] && p_rss_csv=$(join_samples "${p_rss_samples[@]}")
  [[ ${#n_ms_samples[@]} -gt 0 ]] && n_ms_csv=$(join_samples "${n_ms_samples[@]}")
  [[ ${#n_rss_samples[@]} -gt 0 ]] && n_rss_csv=$(join_samples "${n_rss_samples[@]}")
  [[ ${#b_ms_samples[@]} -gt 0 ]] && b_ms_csv=$(join_samples "${b_ms_samples[@]}")
  [[ ${#b_rss_samples[@]} -gt 0 ]] && b_rss_csv=$(join_samples "${b_rss_samples[@]}")
  python3 - "$RESULTS_FILE" "$name" "$correctness_json" \
    "$p_ms_csv" "$p_rss_csv" "$n_ms_csv" "$n_rss_csv" "$b_ms_csv" "$b_rss_csv" <<'PY'
import json
import sys

path, name, correctness_path, *sample_groups = sys.argv[1:]
def samples(raw):
    return [int(value) for value in raw.split(",") if value != ""]

runtime_names = ("perry", "node", "bun")
runtimes = {}
for index, runtime_name in enumerate(runtime_names):
    wall_ms = samples(sample_groups[index * 2])
    rss_kb = samples(sample_groups[index * 2 + 1])
    if wall_ms or rss_kb:
        runtimes[runtime_name] = {"wall_ms": wall_ms, "rss_kb": rss_kb}
with open(correctness_path, encoding="utf-8") as handle:
    correctness = json.load(handle)
with open(path, "a", encoding="utf-8") as handle:
    handle.write(json.dumps({"name": name, "runtimes": runtimes, "correctness": correctness}) + "\n")
PY
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
rm -f "$CURRENT_JSON"
python3 "$BENCHMARK_GATE" build \
  --records "$RESULTS_FILE" \
  --runtime-metadata "$RUNTIME_METADATA" \
  --runs "$RUNS" \
  --expected-benchmarks "$EXPECTED_BENCHMARKS" \
  --output "$CURRENT_JSON"

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
  python3 "$BENCHMARK_GATE" compare "$BASELINE" "$CURRENT_JSON" \
    --speed-threshold "$SPEED_THRESHOLD" \
    --memory-threshold "$MEMORY_THRESHOLD"
  COMPARE_EXIT=$?
  set -e

  if [[ $COMPARE_EXIT -eq 1 && $WARN_ONLY -eq 1 ]]; then
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

exit ${COMPARE_EXIT:-0}
