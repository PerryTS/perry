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
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/wasi_run.py" "${1:?usage: $0 <perry built with --features target-wasi>}"
