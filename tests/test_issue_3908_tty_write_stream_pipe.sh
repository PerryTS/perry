#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
FIXTURE="$ROOT/tests/issue_3908_tty_write_stream_pipe.js"
WORKDIR="${TMPDIR:-/tmp}/perry-issue-3908-$$"
BIN="$WORKDIR/perry-tty-pipe"
NODE_OUT="$WORKDIR/node.out"
PERRY_OUT="$WORKDIR/perry.out"
COMPILE_LOG="$WORKDIR/compile.log"

mkdir -p "$WORKDIR"
trap 'rm -rf "$WORKDIR"' EXIT

if [[ ! -x "$PERRY" ]]; then
  PERRY="$ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
  echo "Perry binary not found; build target/release/perry or target/debug/perry first" >&2
  exit 1
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

set +e
run_with_timeout 10 node "$FIXTURE" 2>&1 | cat >"$NODE_OUT"
node_rc=${PIPESTATUS[0]}
set -e
if [[ "$node_rc" -ne 0 ]]; then
  echo "Node reference run failed" >&2
  cat "$NODE_OUT" >&2
  exit 1
fi

env PERRY_ALLOW_UNIMPLEMENTED=1 "$PERRY" compile --no-cache "$FIXTURE" -o "$BIN" \
  >"$COMPILE_LOG" 2>&1 || {
  cat "$COMPILE_LOG" >&2
  exit 1
}
set +e
run_with_timeout 10 "$BIN" 2>&1 | cat >"$PERRY_OUT"
perry_rc=${PIPESTATUS[0]}
set -e
if [[ "$perry_rc" -ne 0 ]]; then
  echo "Perry fixture run failed" >&2
  cat "$PERRY_OUT" >&2
  exit 1
fi

if ! diff -u "$NODE_OUT" "$PERRY_OUT"; then
  echo "Perry output differed from Node under pipe-backed stdout/stderr" >&2
  exit 1
fi
