#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
FIXTURE="$ROOT/tests/issue_process_stdio_write_stream_shape.js"
WORKDIR="${TMPDIR:-/tmp}/perry-process-stdio-write-shape-$$"
BIN="$WORKDIR/perry-process-stdio-write-shape"
COMPILE_LOG="$WORKDIR/compile.log"
STDOUT_LOG="$WORKDIR/stdout.log"
STDERR_LOG="$WORKDIR/stderr.log"
EXPECTED_STDOUT="$WORKDIR/expected.stdout"
EXPECTED_STDERR="$WORKDIR/expected.stderr"

mkdir -p "$WORKDIR"
trap 'rm -rf "$WORKDIR"' EXIT

if [[ ! -x "$PERRY" ]]; then
  PERRY="$ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
  echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
  exit 0
fi

run_with_timeout() {
  local secs="$1"
  shift
  if command -v timeout >/dev/null 2>&1; then
    timeout "$secs" "$@"
    return $?
  fi
  if command -v gtimeout >/dev/null 2>&1; then
    gtimeout "$secs" "$@"
    return $?
  fi
  "$@" &
  local pid=$!
  (sleep "$secs" && kill -TERM "$pid" 2>/dev/null && sleep 1 && kill -KILL "$pid" 2>/dev/null) &
  local watcher=$!
  if wait "$pid" 2>/dev/null; then
    kill -TERM "$watcher" 2>/dev/null || true
    wait "$watcher" 2>/dev/null || true
    return 0
  fi
  local rc=$?
  kill -TERM "$watcher" 2>/dev/null || true
  wait "$watcher" 2>/dev/null || true
  [[ "$rc" == "143" ]] && return 124
  return "$rc"
}

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_RUNTIME_DIR="$RUNTIME_DIR" "$PERRY" compile --no-cache --no-auto-optimize "$FIXTURE" -o "$BIN" \
  >"$COMPILE_LOG" 2>&1 || {
    cat "$COMPILE_LOG" >&2
    exit 1
  }

set +e
run_with_timeout 10 "$BIN" >"$STDOUT_LOG" 2>"$STDERR_LOG"
run_rc=$?
set -e
if [[ "$run_rc" -ne 0 ]]; then
  echo "Perry stdio write fixture failed with exit code $run_rc" >&2
  echo "--- stdout ---" >&2
  cat "$STDOUT_LOG" >&2
  echo "--- stderr ---" >&2
  cat "$STDERR_LOG" >&2
  exit 1
fi

printf 'stdout-direct\nconsole-stdout\nstdout-async\n' >"$EXPECTED_STDOUT"
printf 'stderr-direct\nconsole-stderr\nstderr-async\n' >"$EXPECTED_STDERR"

diff -u "$EXPECTED_STDOUT" "$STDOUT_LOG"
diff -u "$EXPECTED_STDERR" "$STDERR_LOG"
