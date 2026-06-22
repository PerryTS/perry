#!/usr/bin/env bash
# Long-running HTTP server bench: spawn binary, drive oha load, capture
# rps + p50/p95/p99 latencies + peak RSS, kill, emit one row per measured
# run in the same shape as run_bench.sh so summary.py + check_output.py
# work without modification.
#
# Usage:
#   run_http_bench.sh <workload> <language> <run-cmd...> -- <route>
#
# Examples:
#   run_http_bench.sh http_fastify_minimal perry \
#     /path/to/perry_minimal_bin -- /
#   run_http_bench.sh http_fastify_minimal node \
#     node --import tsx /path/to/node/minimal.ts -- /
#
# All args between <run-cmd...> and `--` form the command launched in
# the background; the value after `--` is the route appended to
# http://127.0.0.1:<port>, where <port> is HONEST_BENCH_HTTP_PORT
# (default 18080) — the same var the kernels bind.
#
# Per measured run:
#   workload, language, binary, run, wall_ms, max_rss_kb, exit_code,
#   stdout_first, stdout_last, output_match (always null for HTTP).
#
# Encoding:
#   - `binary` = the run-cmd joined by spaces, truncated to 200 chars
#   - `wall_ms = 1_000_000 / rps` — "lower is better" semantics carry
#                through to summary.py without code change
#   - `stdout_first` synthesizes the metric tokens so check_output.py's
#     TOKEN_RE matches: `rps=N p50_ms=N p95_ms=N p99_ms=N rss_kb=N`

set -uo pipefail

if [[ $# -lt 4 ]]; then
  echo "usage: $0 <workload> <language> <run-cmd...> -- <route>" >&2
  exit 2
fi

WORKLOAD="$1"; shift
LANGUAGE="$1"; shift

# Split CMD ... -- ROUTE
RUN_CMD=()
ROUTE=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "--" ]]; then
    shift
    ROUTE="${1:-/}"
    break
  fi
  RUN_CMD+=("$1")
  shift
done

if [[ ${#RUN_CMD[@]} -eq 0 || -z "$ROUTE" ]]; then
  echo "usage: $0 <workload> <language> <run-cmd...> -- <route>" >&2
  exit 2
fi

WARMUP="${HONEST_BENCH_WARMUP:-1}"
MEASURED="${HONEST_BENCH_MEASURED:-5}"
DURATION="${HONEST_BENCH_HTTP_DURATION:-15s}"
WARMUP_DUR="${HONEST_BENCH_HTTP_WARMUP_DUR:-5s}"
CONC="${HONEST_BENCH_HTTP_CONC:-10}"
PORT="${HONEST_BENCH_HTTP_PORT:-18080}"
OHA="${HONEST_BENCH_OHA:-/tmp/oha}"

if [[ ! -x "$OHA" ]]; then
  echo "ERROR: oha binary not found at $OHA (set HONEST_BENCH_OHA)" >&2
  exit 2
fi

BIN_LABEL=$(printf '%s ' "${RUN_CMD[@]}" | head -c 200)

# ---------------------------------------------------------------------
# Spawn server, wait for TCP listen
# ---------------------------------------------------------------------
SERVER_LOG=$(mktemp)
"${RUN_CMD[@]}" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

cleanup() {
  if kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM "$SERVER_PID" 2>/dev/null || true
    for _ in 1 2 3 4 5; do
      kill -0 "$SERVER_PID" 2>/dev/null || break
      sleep 0.2
    done
    kill -KILL "$SERVER_PID" 2>/dev/null || true
  fi
  rm -f "$SERVER_LOG"
}
trap cleanup EXIT

# Up to 30s to start listening — Perry binaries cold-start fast but node
# + tsx + fastify first-import can take a few seconds.
ready=0
for _ in $(seq 1 60); do
  if curl -fsS --max-time 1 "http://127.0.0.1:${PORT}${ROUTE}" >/dev/null 2>&1; then
    ready=1
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "ERROR: server exited before listening; first 400 chars of log:" >&2
    head -c 400 "$SERVER_LOG" >&2
    exit 2
  fi
  sleep 0.5
done
if [[ "$ready" != 1 ]]; then
  echo "ERROR: server did not start listening on :${PORT}${ROUTE} within 30s" >&2
  head -c 400 "$SERVER_LOG" >&2
  exit 2
fi

URL="http://127.0.0.1:${PORT}${ROUTE}"

# Peak RSS in KiB for the server process. VmHWM is the resident-set high-water
# mark (matches the `max_rss_kb` row field); VmRSS would only report the current
# instantaneous RSS. Linux /proc only — falls back to 0 elsewhere.
read_rss_kb() {
  # END guard so a /proc file present-but-without-VmHWM still prints 0 (never an
  # empty string, which would break the row builder's int(rss_kb)); `|| echo 0`
  # covers the off-Linux / missing-file case where awk exits nonzero.
  awk '/^VmHWM:/ {print $2; f=1; exit} END {if (!f) print 0}' \
    "/proc/${SERVER_PID}/status" 2>/dev/null || echo 0
}

run_one_measurement() {
  local run_idx="$1"
  local json
  json=$(mktemp)

  local start_ns end_ns
  start_ns=$(python3 -c 'import time; print(time.monotonic_ns())')
  # A failed oha invocation is RECORDED as a zero-throughput row (rps=0 ->
  # exit_code 1 below) and the suite continues — matching run.sh's "record the
  # failure, don't abort the suite" contract (run.sh header) and the sibling
  # run_bench.sh. Aborting here would trip run.sh's `set -e` and discard every
  # already-collected row. oha's stderr stays visible (no 2>/dev/null) so the
  # cause is diagnosable; the empty `{}` makes the row builder emit rps=0.
  if ! "$OHA" -z "$DURATION" -c "$CONC" -r 0 --no-tui --output-format json "$URL" >"$json"; then
    echo "WARNING: oha failed for $WORKLOAD / $LANGUAGE (measured run $run_idx); recording a failed row" >&2
    echo '{}' >"$json"
  fi
  end_ns=$(python3 -c 'import time; print(time.monotonic_ns())')

  local rss_kb
  rss_kb=$(read_rss_kb)

  python3 - "$WORKLOAD" "$LANGUAGE" "$BIN_LABEL" "$run_idx" \
           "$start_ns" "$end_ns" "$rss_kb" "$json" <<'PY'
import json, sys

(_, workload, lang, binary, run, start_ns, end_ns, rss_kb, json_path) = sys.argv
with open(json_path) as f:
    data = json.load(f)

rps    = data.get("summary", {}).get("requestsPerSec") or 0.0
p50    = (data.get("latencyPercentiles", {}).get("p50") or 0.0) * 1000.0
p95    = (data.get("latencyPercentiles", {}).get("p95") or 0.0) * 1000.0
p99    = (data.get("latencyPercentiles", {}).get("p99") or 0.0) * 1000.0
status = data.get("statusCodeDistribution", {}) or {}
success_rate = data.get("summary", {}).get("successRate") or 0.0

# "lower is better" encoding for wall_ms — used by summary.py.
wall_ms = 1_000_000.0 / rps if rps > 0 else 1_000_000.0

# Token block parsed by check_output.py's TOKEN_RE: bare key=value pairs
token_line = (
    f"rps={rps:.2f} "
    f"p50_ms={p50:.3f} "
    f"p95_ms={p95:.3f} "
    f"p99_ms={p99:.3f} "
    f"rss_kb={rss_kb} "
    f"success_rate={success_rate:.4f} "
    f"status={','.join(f'{k}={v}' for k,v in sorted(status.items()))}"
)

row = {
    "workload": workload,
    "language": lang,
    "binary": binary.strip(),
    "run": int(run),
    "wall_ms": wall_ms,
    "max_rss_kb": int(rss_kb),
    # Reflect run health like the other workloads' exit_code: a run that served
    # no requests (rps == 0) or dropped >1% of them (e.g. crashes / 5xx under
    # load) is a failure, so downstream tooling doesn't treat it as a clean row.
    "exit_code": 0 if (rps > 0 and success_rate >= 0.99) else 1,
    "stdout_first": token_line[:200],
    "stdout_last":  token_line[-200:],
    "output_match": None,
    "output_match_reason": "http throughput workload — no expected output",
    "rps": rps,
    "p50_ms": p50,
    "p95_ms": p95,
    "p99_ms": p99,
    "success_rate": success_rate,
    "status_codes": status,
}
print(json.dumps(row))
PY

  rm -f "$json"
}

# Warmup runs — discard
for _ in $(seq 1 "$WARMUP"); do
  "$OHA" -z "$WARMUP_DUR" -c "$CONC" -r 0 --no-tui --output-format quiet "$URL" \
      >/dev/null 2>&1 || true
done

# Measured runs — emit JSON per line
for i in $(seq 1 "$MEASURED"); do
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "ERROR: server died before measured run $i; first 400 chars of log:" >&2
    head -c 400 "$SERVER_LOG" >&2
    exit 2
  fi
  run_one_measurement "$i"
done
