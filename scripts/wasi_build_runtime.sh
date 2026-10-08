#!/usr/bin/env bash
#
# Build libperry_runtime.a for wasm32-wasip2 — the archive `perry compile
# --target wasi` links (#11379) — with the same feature set
# `scripts/wasi_check.sh` checks:
#
#   eval "$(./scripts/wasi_toolchain.sh .cache/wasi)"
#   ./scripts/wasi_build_runtime.sh
#
# Needs wasi-sdk's C compiler for the runtime's C sources (CC_wasm32_wasip2
# and friends, as printed by wasi_toolchain.sh). Liveness: requires Cargo to
# report a WASI static-archive artifact, including
# a fresh cache hit (a valid cached build need not change the archive mtime).

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/wasi_build_runtime.py"
