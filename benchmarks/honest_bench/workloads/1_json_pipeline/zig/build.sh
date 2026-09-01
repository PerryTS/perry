#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"

# Keep the compiler caches out of ~/.cache/zig and out of the source tree. The path is stable so
# rebuilds stay warm; point PERRY_ZIG_CACHE_DIR elsewhere for a cold build.
tmp_root="${TMPDIR:-/tmp}"
zig_cache="${PERRY_ZIG_CACHE_DIR:-${tmp_root%/}/perry-zig-cache}"
export ZIG_GLOBAL_CACHE_DIR="$zig_cache/global"
export ZIG_LOCAL_CACHE_DIR="$zig_cache/json_pipeline"

mkdir -p zig-out/bin
target_args=()
if [[ "$(uname)" == "Darwin" ]]; then
  # Zig 0.15.2 does not recognize macOS 26 as a deployment target.
  target_args=(-target aarch64-macos.14.0)
fi
zig build-exe src/main.zig \
  -O ReleaseFast \
  "${target_args[@]}" \
  -lc \
  --name json_pipeline \
  -femit-bin=zig-out/bin/json_pipeline
echo "built: $(du -h zig-out/bin/json_pipeline | cut -f1) zig-out/bin/json_pipeline"
