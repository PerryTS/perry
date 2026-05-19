#!/usr/bin/env bash
#
# App-pattern bench runner. For each kernel in `kernels/`, compile
# with Perry, then hyperfine perry-binary vs `bun run` vs `node
# --experimental-strip-types`, and aggregate min/mean/stddev into a
# markdown matrix at `results/matrix-<timestamp>.md`.
#
# Each kernel must:
#   - run for ~50–500 ms (long enough that hyperfine variance is small,
#     short enough that the full sweep completes in minutes)
#   - print exactly one "checksum: <values>" line on stdout — used as a
#     cross-runtime sanity check (Perry/Bun/Node should agree)
#
# Usage:
#   ./run.sh                    # run all kernels
#   ./run.sh json_parse_1mb     # run a single kernel by basename
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PERRY_BIN="$REPO_ROOT/target/release/perry"
KERNELS_DIR="$SCRIPT_DIR/kernels"
RESULTS_DIR="$SCRIPT_DIR/results"
mkdir -p "$RESULTS_DIR"

if [ ! -x "$PERRY_BIN" ]; then
  echo "Error: perry binary not found at $PERRY_BIN — build first with"
  echo "  cargo build --release -p perry-runtime -p perry-stdlib -p perry"
  exit 1
fi

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "Error: hyperfine not installed (brew install hyperfine)"
  exit 1
fi

TIMEOUT_BIN=""
if command -v timeout >/dev/null 2>&1; then
  TIMEOUT_BIN="$(command -v timeout)"
elif command -v gtimeout >/dev/null 2>&1; then
  TIMEOUT_BIN="$(command -v gtimeout)"
fi

run_smoke() {
  if [ -n "$TIMEOUT_BIN" ]; then
    "$TIMEOUT_BIN" 30 env NO_COLOR=1 FORCE_COLOR=0 "$@" 2>/dev/null
  else
    env NO_COLOR=1 FORCE_COLOR=0 "$@" 2>/dev/null
  fi
}

# Resolve which kernels to run.
if [ $# -gt 0 ]; then
  KERNELS=()
  for arg in "$@"; do
    KERNELS+=("$KERNELS_DIR/$arg.ts")
  done
else
  KERNELS=("$KERNELS_DIR"/*.ts)
fi

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
OUTPUT="$RESULTS_DIR/matrix-$TIMESTAMP.md"

# Initialize markdown
{
  echo "# App-pattern bench matrix — $(date '+%Y-%m-%d %H:%M:%S')"
  echo
  echo "Each cell is mean ms (min ms). Lower is better."
  echo "Final column is **perry/bun ratio** — > 2× is a real gap, < 1.5× is fine."
  echo
  echo "| Kernel | Perry | Bun | Node | perry/bun | perry/node |"
  echo "|---|---:|---:|---:|---:|---:|"
} > "$OUTPUT"

declare -i WIN_COUNT=0
declare -i LOSE_COUNT=0
declare -i SLOW_COUNT=0

for KERNEL in "${KERNELS[@]}"; do
  NAME=$(basename "$KERNEL" .ts)
  echo "=== $NAME ==="

  # Compile with Perry. Skip the kernel on compile failure.
  PERRY_OUT="/tmp/bench-app-pattern-$NAME"
  rm -f "$PERRY_OUT"
  if ! COMPILE_LOG=$("$PERRY_BIN" "$KERNEL" -o "$PERRY_OUT" 2>&1); then
    echo "  ✗ Perry compile failed:"
    echo "$COMPILE_LOG" | tail -10 | sed 's/^/    /'
    {
      echo "| $NAME | **COMPILE FAIL** | – | – | – | – |"
    } >> "$OUTPUT"
    continue
  fi

  # Quick checksum sanity: each runtime should print the same checksum line.
  # Bound each smoke-run by a hard timeout so a Perry hang doesn't stall
  # the whole sweep — emit a row marking the kernel as RUNTIME FAIL and
  # skip the hyperfine pass for it.
  PERRY_SUM=$(run_smoke "$PERRY_OUT" | grep '^checksum:' || echo "MISSING")
  BUN_SUM=$(run_smoke bun run "$KERNEL" | grep '^checksum:' || echo "MISSING")
  NODE_SUM=$(run_smoke node --experimental-strip-types "$KERNEL" | grep '^checksum:' || echo "MISSING")
  if [ "$PERRY_SUM" = "MISSING" ]; then
    echo "  ✗ Perry produced no checksum (hang or empty output):"
    echo "    Bun:   $BUN_SUM"
    echo "    Node:  $NODE_SUM"
    {
      echo "| $NAME | **RUNTIME FAIL** | – | – | – | – |"
    } >> "$OUTPUT"
    continue
  fi
  if [ "$PERRY_SUM" != "$BUN_SUM" ] || [ "$BUN_SUM" != "$NODE_SUM" ]; then
    echo "  ! Checksum mismatch:"
    echo "    Perry: $PERRY_SUM"
    echo "    Bun:   $BUN_SUM"
    echo "    Node:  $NODE_SUM"
  fi

  # Run hyperfine on all three runtimes in one invocation so the runs
  # interleave (less affected by transient system load on any one
  # runtime).
  JSON="/tmp/bench-app-pattern-$NAME.json"
  hyperfine --warmup 3 --runs 15 --time-unit millisecond \
    --export-json "$JSON" \
    --command-name "perry"  "$PERRY_OUT" \
    --command-name "bun"    "bun run $KERNEL" \
    --command-name "node"   "node --experimental-strip-types $KERNEL" \
    >/dev/null 2>&1

  # Extract mean / min for each runtime.
  read PERRY_MEAN PERRY_MIN < <(python3 -c "
import json,sys
d=json.load(open('$JSON'))
r=d['results'][0]
print(f\"{r['mean']*1000:.1f} {r['min']*1000:.1f}\")
")
  read BUN_MEAN BUN_MIN < <(python3 -c "
import json,sys
d=json.load(open('$JSON'))
r=d['results'][1]
print(f\"{r['mean']*1000:.1f} {r['min']*1000:.1f}\")
")
  read NODE_MEAN NODE_MIN < <(python3 -c "
import json,sys
d=json.load(open('$JSON'))
r=d['results'][2]
print(f\"{r['mean']*1000:.1f} {r['min']*1000:.1f}\")
")

  PB_RATIO=$(python3 -c "print(f'{$PERRY_MEAN / $BUN_MEAN:.2f}x')")
  PN_RATIO=$(python3 -c "print(f'{$PERRY_MEAN / $NODE_MEAN:.2f}x')")
  PB_RATIO_NUM=$(python3 -c "print($PERRY_MEAN / $BUN_MEAN)")

  # Classify
  CLASS=""
  if python3 -c "import sys; sys.exit(0 if $PB_RATIO_NUM < 1.0 else 1)" 2>/dev/null; then
    CLASS="✅ win"; WIN_COUNT=$((WIN_COUNT+1))
  elif python3 -c "import sys; sys.exit(0 if $PB_RATIO_NUM < 1.5 else 1)" 2>/dev/null; then
    CLASS="✓ ok"
  elif python3 -c "import sys; sys.exit(0 if $PB_RATIO_NUM < 2.0 else 1)" 2>/dev/null; then
    CLASS="⚠ borderline"; LOSE_COUNT=$((LOSE_COUNT+1))
  else
    CLASS="✗ slow"; SLOW_COUNT=$((SLOW_COUNT+1))
  fi

  echo "  perry $PERRY_MEAN ms  bun $BUN_MEAN ms  node $NODE_MEAN ms  perry/bun=$PB_RATIO  $CLASS"

  {
    echo "| $NAME | $PERRY_MEAN ($PERRY_MIN) | $BUN_MEAN ($BUN_MIN) | $NODE_MEAN ($NODE_MIN) | $PB_RATIO $CLASS | $PN_RATIO |"
  } >> "$OUTPUT"
done

{
  echo
  echo "## Summary"
  echo
  echo "- Perry-faster-than-Bun kernels: **$WIN_COUNT**"
  echo "- Borderline (1.5–2× slower): **$LOSE_COUNT**"
  echo "- Slow (≥ 2× slower): **$SLOW_COUNT**"
  echo
  echo "**Priority backlog:** the ✗ slow rows are the gaps. Each one is a focused workstream."
} >> "$OUTPUT"

echo
echo "=== Done ==="
echo "Results: $OUTPUT"
cat "$OUTPUT"
