#!/usr/bin/env bash
# Run a single benchmark binary with 5 warmup + 20 measured runs.
#
# Emits one JSON object per MEASURED run to stdout; the top-level driver
# concatenates these into results/results.json. Warmup runs are discarded.
#
# Usage:
#   run_bench.sh <workload> <language> <binary_path> [args...]
#
# Captured per run:
#   workload       — e.g. "image_convolution"
#   language       — "rust" | "zig" | "perry" | "node" | "bun"
#   binary         — path
#   run            — 1..MEASURED
#   wall_ms        — python time.monotonic_ns() delta, millis
#   max_rss_kb     — macOS `/usr/bin/time -l` peak memory footprint, kB
#   exit_code
#   stdout_first   — first 200 chars of stdout (truncated)
#   stdout_last    — last  200 chars of stdout (truncated)
#   output_match           — bool|null (#441 — null when no expected entry yet)
#   output_match_reason    — humanized diff if false; "" if true; descriptor if null
#
# Output-correctness gate (#441) is opt-in via env vars:
#   HONEST_BENCH_EXPECTED_JSON  — path to results/expected.json
#   HONEST_BENCH_CHECK_OUTPUT   — path to harness/check_output.py
#   HONEST_BENCH_OUTPUT_FILE    — path the run produces (sha256-compared)

set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "usage: $0 <workload> <language> <binary> [args...]" >&2
  exit 2
fi

WORKLOAD="$1"; shift
LANGUAGE="$1"; shift
BINARY="$1"; shift

WARMUP="${HONEST_BENCH_WARMUP:-5}"
MEASURED="${HONEST_BENCH_MEASURED:-20}"

measure_once() {
  local run="$1"; shift
  local kind="$1"; shift
  local tmp_err tmp_out
  tmp_err=$(mktemp)
  tmp_out=$(mktemp)

  local start_ns end_ns
  start_ns=$(python3 -c 'import time; print(time.monotonic_ns())')
  set +e
  /usr/bin/time -l "$BINARY" "$@" >"$tmp_out" 2>"$tmp_err"
  local exit_code=$?
  set -e
  end_ns=$(python3 -c 'import time; print(time.monotonic_ns())')

  local wall_ns=$((end_ns - start_ns))

  # macOS `time -l` stderr: peak memory footprint is in BYTES despite what
  # `man time` implies on older macs. Convert to kB.
  local peak_mem
  peak_mem=$(awk '/peak memory footprint/ {print $1; exit}' "$tmp_err" 2>/dev/null || echo 0)
  [[ -z "$peak_mem" ]] && peak_mem=0
  local peak_kb=$((peak_mem / 1024))

  local stdout_first stdout_last
  stdout_first=$(head -1 "$tmp_out" 2>/dev/null | head -c 200 || true)
  stdout_last=$(tail -1  "$tmp_out" 2>/dev/null | head -c 200 || true)

  if [[ "$kind" == "measured" ]]; then
    python3 - \
        "$WORKLOAD" "$LANGUAGE" "$BINARY" "$run" "$wall_ns" "$peak_kb" \
        "$exit_code" "$stdout_first" "$stdout_last" "$tmp_out" <<'PY'
import json, os, subprocess, sys
(_, workload, lang, binary, run, wall_ns, peak_kb,
 exit_code, stdout_first, stdout_last, stdout_path) = sys.argv

row = {
    "workload": workload,
    "language": lang,
    "binary": binary,
    "run": int(run),
    "wall_ms": int(wall_ns) / 1_000_000.0,
    "max_rss_kb": int(peak_kb),
    "exit_code": int(exit_code),
    "stdout_first": stdout_first,
    "stdout_last": stdout_last,
}

# #441: output-correctness gate.
expected = os.environ.get("HONEST_BENCH_EXPECTED_JSON", "")
checker  = os.environ.get("HONEST_BENCH_CHECK_OUTPUT", "")
output_file = os.environ.get("HONEST_BENCH_OUTPUT_FILE", "")
if expected and checker and os.path.exists(expected) and os.path.exists(checker):
    cmd = [sys.executable, checker,
           "--expected-json", expected,
           "--workload", workload,
           "--stdout-file", stdout_path]
    if output_file:
        cmd += ["--output-file", output_file]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if r.returncode == 0 and r.stdout.strip():
            check = json.loads(r.stdout.strip())
            row["output_match"] = check.get("output_match")
            row["output_match_reason"] = check.get("output_match_reason", "")
        else:
            row["output_match"] = None
            row["output_match_reason"] = f"checker exit {r.returncode}: {r.stderr.strip()[:120]}"
    except Exception as e:
        row["output_match"] = None
        row["output_match_reason"] = f"check error: {type(e).__name__}: {e}"

print(json.dumps(row))
PY
  fi
  rm -f "$tmp_err" "$tmp_out"
}

# Warmup — discard
for i in $(seq 1 "$WARMUP"); do
  measure_once "$i" "warmup" "$@" >/dev/null 2>&1 || true
done
# Measured — emit
for i in $(seq 1 "$MEASURED"); do
  measure_once "$i" "measured" "$@"
done
