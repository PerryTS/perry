#!/usr/bin/env bash
# Quick benchmark — runs 5 fast benchmarks in ~15 seconds
# Reports speed ratio vs Node AND peak RSS
#
# Usage: ./benchmarks/quick.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SUITE_DIR="$SCRIPT_DIR/suite"
COMPILETS="$ROOT/target/release/perry"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

if [[ ! -f "$COMPILETS" ]]; then
  echo "Building Perry..."
  (cd "$ROOT" && cargo build --release --quiet)
fi

BENCHMARKS="05_fibonacci.ts 06_math_intensive.ts 10_nested_loops.ts 13_factorial.ts 16_matrix_multiply.ts"
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
  echo "Node.js is unavailable for .ts benchmark inputs; Node columns will be skipped." >&2
fi

extract_time() {
  awk -F: '/^[a-z_]+:[0-9]+/ {print $2; exit}' <<<"$1"
}

measure() {
  local tmp_err=$(mktemp) tmp_out=$(mktemp)
  if [[ -x /usr/bin/time ]]; then
    if [[ "$(uname)" == "Darwin" ]]; then
      /usr/bin/time -l "$@" >"$tmp_out" 2>"$tmp_err" || true
    else
      /usr/bin/time -v "$@" >"$tmp_out" 2>"$tmp_err" || true
    fi
  else
    "$@" >"$tmp_out" 2>"$tmp_err" || true
  fi
  local rss_mb=0
  if [[ "$(uname)" == "Darwin" ]]; then
    local rss_bytes
    rss_bytes=$(awk '/peak memory footprint/ {print $1; exit} /maximum resident set size/ {print $1; exit}' "$tmp_err" 2>/dev/null || true)
    rss_bytes=${rss_bytes:-0}
    rss_mb=$((rss_bytes / 1024 / 1024))
  else
    local rss_kb
    rss_kb=$(awk -F': ' '/Maximum resident set size/ {print $2; exit}' "$tmp_err" 2>/dev/null || true)
    rss_kb=${rss_kb:-0}
    rss_mb=$((rss_kb / 1024))
  fi
  local output
  output=$(cat "$tmp_out")
  rm -f "$tmp_err" "$tmp_out"
  printf '%s\n%s\n' "$rss_mb" "$output"
}

echo -e "${BOLD}${CYAN}Quick Bench (5 benchmarks)${NC}"
echo ""

# Compile
cd "$SUITE_DIR"
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  "$COMPILETS" "$bench" -o "$name" 2>/dev/null || echo "FAIL: $bench"
done

printf "${BOLD}%-18s %8s %8s %8s %8s %8s %8s${NC}\n" \
  "Benchmark" "Perry" "Node" "Ratio" "P-RSS" "N-RSS" "MemR"
echo "───────────────────────────────────────────────────────────────────"

for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  display=$(echo "$name" | sed 's/^[0-9]*_//')

  # Perry
  result=$(measure "./$name")
  p_rss=$(sed -n '1p' <<<"$result")
  p_out=$(sed '1d' <<<"$result")
  p_ms=$(extract_time "$p_out")

  # Node
  n_ms="-"; n_rss="-"
  ratio="-"; mratio="-"
  if [[ $HAS_NODE -eq 1 ]]; then
    result=$(measure "${NODE_CMD[@]}" "$bench")
    n_rss=$(sed -n '1p' <<<"$result")
    n_out=$(sed '1d' <<<"$result")
    n_ms=$(extract_time "$n_out")

    if [[ "$p_ms" =~ ^[0-9]+$ && "$n_ms" =~ ^[0-9]+$ && "$n_ms" -gt 0 ]]; then
      ratio=$(python3 -c "print(f'{$p_ms/$n_ms:.2f}x')")
      if (( p_ms < n_ms )); then
        ratio="${GREEN}${ratio}${NC}"
      else
        ratio="${RED}${ratio}${NC}"
      fi
    fi
    if [[ "$p_rss" =~ ^[0-9]+$ && "$n_rss" =~ ^[0-9]+$ && "$n_rss" -gt 0 ]]; then
      mratio=$(python3 -c "print(f'{$p_rss/$n_rss:.2f}x')")
    fi
  fi

  printf "%-18s %7sms %7sms %8b %6sMB %6sMB %8s\n" \
    "$display" "$p_ms" "$n_ms" "$ratio" "$p_rss" "$n_rss" "$mratio"

  rm -f "$SUITE_DIR/$name"
done
echo ""
