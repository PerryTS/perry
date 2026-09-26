#!/usr/bin/env bash
#
# Compile the programs in scripts/wasi_smoke/ with `perry compile --target
# wasi`, run each under wasmtime and compare stdout plus the exit status with
# the checked-in `.out` (Node's output for the same program; #11379):
#
#   cargo build --profile perry-dev -p perry --features target-wasi
#   ./scripts/wasi_build_runtime.sh          # libperry_runtime.a for wasm32-wasip2
#   ./scripts/wasi_smoke.sh target/perry-dev/perry
#
# Needs wasi-sdk (WASI_SDK_PATH) and wasmtime (WASMTIME, else on PATH);
# `scripts/wasi_toolchain.sh` installs the pinned versions.
#
# Liveness: every program must produce a component the WASI host runs, and a
# run with no programs fails, so a green run cannot be vacuous.

set -euo pipefail

PERRY="$(realpath "${1:?usage: $0 <perry built with --features target-wasi>}")"
WASMTIME="${WASMTIME:-wasmtime}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

n=0
failed=()
for src in "$ROOT"/scripts/wasi_smoke/*.ts; do
  name="$(basename "$src" .ts)"
  expected="${src%.ts}.out"
  wasm="$OUT/$name.wasm"
  if ! "$PERRY" compile "$src" -o "$wasm" --target wasi >"$OUT/$name.log" 2>&1; then
    echo "::error::$name: compile/link failed" >&2
    tail -20 "$OUT/$name.log" >&2
    failed+=("$name")
    continue
  fi
  status=0
  "$WASMTIME" run "$wasm" >"$OUT/$name.stdout" 2>"$OUT/$name.stderr" || status=$?
  echo "exit $status" >>"$OUT/$name.stdout"
  if ! diff -u "$expected" "$OUT/$name.stdout" >"$OUT/$name.diff"; then
    echo "::error::$name: output differs from $expected" >&2
    cat "$OUT/$name.diff" "$OUT/$name.stderr" >&2
    failed+=("$name")
    continue
  fi
  echo "ok   $name"
  n=$((n + 1))
done

if ((${#failed[@]})); then
  echo "FAILED: ${failed[*]}" >&2
  exit 1
fi
if ((n == 0)); then
  echo "::error::no smoke programs ran" >&2
  exit 1
fi
echo "OK: $n programs ran under $("$WASMTIME" --version) with Node's output"
