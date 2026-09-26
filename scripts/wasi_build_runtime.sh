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
# and friends, as printed by wasi_toolchain.sh). Liveness: fails unless the
# archive exists afterwards and is newer than the start of the build.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
: "${CC_wasm32_wasip2:?set up wasi-sdk first: eval \"\$(./scripts/wasi_toolchain.sh)\"}"

features="$(./scripts/wasi_check.sh --print-features | tr ',' '\n' | sed 's|^|perry-runtime/|' | paste -sd, -)"
archive="target/wasm32-wasip2/release/libperry_runtime.a"
stamp="$(mktemp)"
trap 'rm -f "$stamp"' EXIT

echo "== libperry_runtime.a for wasm32-wasip2"
echo "   features: $features"
cargo build --locked --release -p perry-runtime-static --target wasm32-wasip2 \
  --no-default-features --features "$features"

if [[ ! "$archive" -nt "$stamp" ]]; then
  echo "::error::$archive was not (re)built" >&2
  exit 1
fi
echo "OK: $archive"
