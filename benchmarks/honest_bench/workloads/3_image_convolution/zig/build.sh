#!/bin/bash
# Zig 0.15.2 doesn't recognize macOS 26 as a valid target; pin to 14.0.
# Using `zig build-exe` directly because `zig build` bootstraps the build
# script against the host target, which has the same version mismatch.
set -euo pipefail
cd "$(dirname "$0")"

# Keep the compiler caches out of ~/.cache/zig and out of the source tree. The path is stable so
# rebuilds stay warm; point PERRY_ZIG_CACHE_DIR elsewhere for a cold build.
tmp_root="${TMPDIR:-/tmp}"
zig_cache="${PERRY_ZIG_CACHE_DIR:-${tmp_root%/}/perry-zig-cache}"
export ZIG_GLOBAL_CACHE_DIR="$zig_cache/global"
export ZIG_LOCAL_CACHE_DIR="$zig_cache/image_conv"

mkdir -p zig-out/bin
zig build-exe src/main.zig \
  -O ReleaseFast \
  -target aarch64-macos.14.0 \
  -lc \
  --name image_conv \
  -femit-bin=zig-out/bin/image_conv
echo "built: $(du -h zig-out/bin/image_conv | cut -f1) zig-out/bin/image_conv"
