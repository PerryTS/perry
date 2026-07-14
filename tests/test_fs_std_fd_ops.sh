#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
FIXTURE="$ROOT/tests/issue_fs_std_fd_ops.js"
WORKDIR="${TMPDIR:-/tmp}/perry-fs-std-fd-ops-$$"
BIN="$WORKDIR/perry-fs-std-fd-ops"
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

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_RUNTIME_DIR="$RUNTIME_DIR" "$PERRY" compile --no-cache --no-auto-optimize "$FIXTURE" -o "$BIN" \
  >"$COMPILE_LOG" 2>&1 || {
    cat "$COMPILE_LOG" >&2
    exit 1
  }

set +e
"$BIN" >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null
run_rc=$?
set -e
if [[ "$run_rc" -ne 0 ]]; then
  echo "Perry fs std-fd fixture failed with exit code $run_rc" >&2
  echo "--- stdout ---" >&2
  cat "$STDOUT_LOG" >&2
  echo "--- stderr ---" >&2
  cat "$STDERR_LOG" >&2
  exit 1
fi

printf 'stdout-fd-write\nstdout-fd-buffer\ndone\n' >"$EXPECTED_STDOUT"
printf 'stderr-fd-write\n' >"$EXPECTED_STDERR"

diff -u "$EXPECTED_STDOUT" "$STDOUT_LOG"
diff -u "$EXPECTED_STDERR" "$STDERR_LOG"
