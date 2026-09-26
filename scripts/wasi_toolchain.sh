#!/usr/bin/env bash
#
# Install the pinned WASI toolchain (#11379) into a directory and print the
# environment that points the build at it:
#
#   eval "$(./scripts/wasi_toolchain.sh .cache/wasi)"
#
#   wasi-sdk   clang + wasm-ld + the wasm32-wasip2 sysroot: compiles the
#              runtime's C for WASI and links `perry compile --target wasi`.
#   wasmtime   the WASI host the smoke test runs programs under. Perry's
#              closure dispatch uses `ref.test` (the GC proposal) on WASI;
#              wasmtime enables it by default.
#
# Versions are adopted only once they are older than the repo's soak window
# (scripts/soak/constants.mts): wasi-sdk 34 was published 2026-08-25 and
# wasmtime 48.0.0 on 2026-08-20. Each archive is verified against its
# sha256 before it is unpacked.

set -euo pipefail

DEST="$(mkdir -p "${1:-.cache/wasi}" && cd "${1:-.cache/wasi}" && pwd)"

WASI_SDK_VERSION=34
WASI_SDK_SHA256=b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4
WASMTIME_VERSION=48.0.0
WASMTIME_SHA256=1d23a692da51a4f825698f3f999da71f28bad19a96df5395fadc8d07f162dac3

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
  echo "::error::only x86_64 Linux is pinned; install wasi-sdk $WASI_SDK_VERSION and wasmtime $WASMTIME_VERSION by hand" >&2
  exit 1
fi

fetch() { # url sha256 file
  if [[ ! -f "$DEST/$3" ]] || ! echo "$2  $DEST/$3" | sha256sum -c --status; then
    curl -fsSL --retry 3 -o "$DEST/$3.part" "$1"
    echo "$2  $DEST/$3.part" | sha256sum -c --quiet >&2 ||
      { echo "::error::$3 does not match its pinned sha256" >&2; exit 1; }
    mv "$DEST/$3.part" "$DEST/$3"
  fi
}

sdk="wasi-sdk-$WASI_SDK_VERSION.0-x86_64-linux"
fetch "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$WASI_SDK_VERSION/$sdk.tar.gz" \
  "$WASI_SDK_SHA256" "$sdk.tar.gz"
[[ -x "$DEST/$sdk/bin/clang" ]] || tar -xzf "$DEST/$sdk.tar.gz" -C "$DEST"

wt="wasmtime-v$WASMTIME_VERSION-x86_64-linux"
fetch "https://github.com/bytecodealliance/wasmtime/releases/download/v$WASMTIME_VERSION/$wt.tar.xz" \
  "$WASMTIME_SHA256" "$wt.tar.xz"
[[ -x "$DEST/$wt/wasmtime" ]] || tar -xJf "$DEST/$wt.tar.xz" -C "$DEST"

cat <<ENV
export WASI_SDK_PATH='$DEST/$sdk'
export WASMTIME='$DEST/$wt/wasmtime'
export CC_wasm32_wasip2='$DEST/$sdk/bin/clang'
export AR_wasm32_wasip2='$DEST/$sdk/bin/llvm-ar'
export CFLAGS_wasm32_wasip2='--target=wasm32-wasip2 --sysroot=$DEST/$sdk/share/wasi-sysroot'
ENV
