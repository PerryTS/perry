#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
FIXTURE="$ROOT/tests/integration/issue_fs_inherited_fd.js"
CLOSE_FIXTURE="$ROOT/tests/integration/issue_fs_inherited_fd_close.js"
WORKDIR="${TMPDIR:-/tmp}/perry-fs-inherited-fd-$$"
BIN="$WORKDIR/perry-fs-inherited-fd"
CLOSE_BIN="$WORKDIR/perry-fs-inherited-fd-close"
COMPILE_LOG="$WORKDIR/compile.log"
STDOUT_LOG="$WORKDIR/stdout.log"
STDERR_LOG="$WORKDIR/stderr.log"
EXPECTED_STDOUT="$WORKDIR/expected.stdout"

mkdir -p "$WORKDIR"
trap 'rm -rf "$WORKDIR"' EXIT

if [[ ! -x "$PERRY" ]]; then
  PERRY="$ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
  echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
  exit 0
fi
case "$(uname -s)" in
  Linux|Darwin|*BSD) ;;
  *) echo "SKIP: inherited descriptors are tested on Unix"; exit 0 ;;
esac

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_RUNTIME_DIR="$RUNTIME_DIR" "$PERRY" compile --no-cache --no-auto-optimize "$FIXTURE" -o "$BIN" \
  >"$COMPILE_LOG" 2>&1 || {
    cat "$COMPILE_LOG" >&2
    exit 1
  }

printf 'three\n' >"$WORKDIR/three.txt"
printf 'seven\n' >"$WORKDIR/seven.txt"

set +e
"$BIN" >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null 3<"$WORKDIR/three.txt" 7<"$WORKDIR/seven.txt"
run_rc=$?
set -e
if [[ "$run_rc" -ne 0 ]]; then
  echo "Perry inherited-fd fixture failed with exit code $run_rc" >&2
  echo "--- stdout ---" >&2
  cat "$STDOUT_LOG" >&2
  echo "--- stderr ---" >&2
  cat "$STDERR_LOG" >&2
  exit 1
fi

printf 'fd3: three\nfd7: seven\ndone\n' >"$EXPECTED_STDOUT"
diff -u "$EXPECTED_STDOUT" "$STDOUT_LOG"

# closeSync(4) on an inherited pipe write end must close the descriptor itself:
# the reader of the fifo sees EOF while the program is still running.
env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_RUNTIME_DIR="$RUNTIME_DIR" "$PERRY" compile --no-cache --no-auto-optimize "$CLOSE_FIXTURE" -o "$CLOSE_BIN" \
  >"$WORKDIR/compile-close.log" 2>&1 || {
    cat "$WORKDIR/compile-close.log" >&2
    exit 1
  }
mkfifo "$WORKDIR/fifo"
( cat "$WORKDIR/fifo" >/dev/null; : >"$WORKDIR/eof" ) &
reader=$!
set +e
"$CLOSE_BIN" "$WORKDIR/eof" >"$WORKDIR/close.out" 2>"$WORKDIR/close.err" </dev/null 4>"$WORKDIR/fifo"
close_rc=$?
set -e
if [[ "$close_rc" -ne 0 ]]; then
  kill "$reader" 2>/dev/null || true
  echo "closeSync on an inherited descriptor left it open (exit $close_rc)" >&2
  cat "$WORKDIR/close.err" >&2
  exit 1
fi
wait "$reader"
diff -u <(printf 'eof\n') "$WORKDIR/close.out"
