#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
FIXTURE="$ROOT/tests/issue_fs_nonblocking_read.js"
WORKDIR="${TMPDIR:-/tmp}/perry-fs-nonblocking-read-$$"
BIN="$WORKDIR/perry-fs-nonblocking-read"
COMPILE_LOG="$WORKDIR/compile.log"
STDOUT_LOG="$WORKDIR/stdout.log"
STDERR_LOG="$WORKDIR/stderr.log"

mkdir -p "$WORKDIR"
trap 'rm -rf "$WORKDIR"' EXIT

if [[ ! -x "$PERRY" ]]; then
  PERRY="$ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
  echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
  exit 0
fi
if ! command -v mkfifo >/dev/null 2>&1; then
  echo "SKIP: mkfifo not available"
  exit 0
fi

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_RUNTIME_DIR="$RUNTIME_DIR" "$PERRY" compile --no-cache --no-auto-optimize "$FIXTURE" -o "$BIN" \
  >"$COMPILE_LOG" 2>&1 || {
    cat "$COMPILE_LOG" >&2
    exit 1
  }

# A blocking descriptor parks in readSync forever — cap the run so the
# regression shows up as a failure rather than a hung suite.
set +e
if command -v timeout >/dev/null 2>&1; then
  timeout 20 "$BIN" >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null
elif command -v gtimeout >/dev/null 2>&1; then
  gtimeout 20 "$BIN" >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null
else
  "$BIN" >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null
fi
run_rc=$?
set -e

if [[ "$run_rc" -eq 124 ]]; then
  echo "readSync on a non-blocking fd hung (O_NONBLOCK was dropped on open)" >&2
  exit 1
fi
if [[ "$run_rc" -ne 0 ]]; then
  echo "Perry non-blocking read fixture failed with exit code $run_rc" >&2
  cat "$STDOUT_LOG" "$STDERR_LOG" >&2
  exit 1
fi

grep -qx "nonblocking-read ok" "$STDOUT_LOG" || {
  echo "unexpected output:" >&2
  cat "$STDOUT_LOG" >&2
  exit 1
}
