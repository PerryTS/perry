#!/usr/bin/env bash
#
# Install the pinned WASI toolchain (#11379) into a directory and print the
# environment that points the build at it:
#
#   eval "$(./scripts/wasi_toolchain.sh .cache/wasi)"
#
#   wasi-sdk   clang + wasm-ld + the wasm32-wasip2 sysroot: compiles the
#              runtime's C for WASI and links `perry compile --target wasi`.
#   wasmtime   the WASI host the smoke test runs programs under.
#
# Versions are adopted only once they are older than the repo's soak window
# (scripts/soak/constants.mts): wasi-sdk 34 was published 2026-08-25 and
# wasmtime 48.0.0 on 2026-08-20. Each archive is verified against its
# sha256 before it is unpacked. Pins are the release asset digests from:
# https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34
# https://github.com/bytecodealliance/wasmtime/releases/tag/v48.0.0

set -euo pipefail

DEST="$(mkdir -p "${1:-.cache/wasi}" && cd "${1:-.cache/wasi}" && pwd)"

WASI_SDK_VERSION=34
WASMTIME_VERSION=48.0.0

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    sdk_platform=x86_64-linux; wt_platform=x86_64-linux
    WASI_SDK_SHA256=b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4
    WASMTIME_SHA256=1d23a692da51a4f825698f3f999da71f28bad19a96df5395fadc8d07f162dac3
    ;;
  Linux-aarch64|Linux-arm64)
    sdk_platform=arm64-linux; wt_platform=aarch64-linux
    WASI_SDK_SHA256=f7e243dff54d60bcc576e94d6166b69f410f2500ae4a9ceef34315be10e77971
    WASMTIME_SHA256=ea20f14785f908afcd07063ade8d89b82b15228d9e6be426ea12ebecbc9c0c8b
    ;;
  Darwin-arm64)
    sdk_platform=arm64-macos; wt_platform=aarch64-macos
    WASI_SDK_SHA256=9c59398106b417f8f14913380fdf0097a8cc0ff4af9eb3ce0065a859e88d49e9
    WASMTIME_SHA256=b62f687c7b19dd7c04b44ba5d6170a92a2c99bfc190887348aee9323e3703e60
    ;;
  Darwin-x86_64)
    sdk_platform=x86_64-macos; wt_platform=x86_64-macos
    WASI_SDK_SHA256=87d27fa8adc68dee59bfbf2e22a6d34ef717c34d6bf1d8af2a56fc929d9ce0eb
    WASMTIME_SHA256=3cf3c8d1f67184183acd61567e758046d40dddd169295dc9e7570b2b20505ac9
    ;;
  *)
    echo "::error::unsupported Unix host; use x86_64/ARM64 Linux or macOS, or wasi_toolchain.ps1 on Windows" >&2
    exit 1
    ;;
esac

verify() {
  local digest
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum < "$2")"
  else
    digest="$(shasum -a 256 < "$2")"
  fi
  [[ "${digest%% *}" == "$1" ]]
}

fetch() { # url sha256 file
  if [[ ! -f "$DEST/$3" ]] || ! verify "$2" "$DEST/$3"; then
    curl -fsSL --retry 3 -o "$DEST/$3.part" "$1"
    verify "$2" "$DEST/$3.part" ||
      { echo "::error::$3 does not match its pinned sha256" >&2; exit 1; }
    mv "$DEST/$3.part" "$DEST/$3"
  fi
}

sdk="wasi-sdk-$WASI_SDK_VERSION.0-$sdk_platform"
fetch "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$WASI_SDK_VERSION/$sdk.tar.gz" \
  "$WASI_SDK_SHA256" "$sdk.tar.gz"
[[ -x "$DEST/$sdk/bin/clang" ]] || tar -xzf "$DEST/$sdk.tar.gz" -C "$DEST"

wt="wasmtime-v$WASMTIME_VERSION-$wt_platform"
fetch "https://github.com/bytecodealliance/wasmtime/releases/download/v$WASMTIME_VERSION/$wt.tar.xz" \
  "$WASMTIME_SHA256" "$wt.tar.xz"
[[ -x "$DEST/$wt/wasmtime" ]] || tar -xJf "$DEST/$wt.tar.xz" -C "$DEST"

# This output is evaluated by bash callers; quote paths including spaces,
# apostrophes and shell metacharacters without changing their values.
printf 'export WASI_SDK_PATH=%q\n' "$DEST/$sdk"
printf 'export WASMTIME=%q\n' "$DEST/$wt/wasmtime"
printf 'export CC_wasm32_wasip2=%q\n' "$DEST/$sdk/bin/clang"
printf 'export AR_wasm32_wasip2=%q\n' "$DEST/$sdk/bin/llvm-ar"
printf -v wasi_sysroot_flag '%q' "--sysroot=$DEST/$sdk/share/wasi-sysroot"
printf 'export CFLAGS_wasm32_wasip2=%q\n' "--target=wasm32-wasip2 $wasi_sysroot_flag"
printf 'export CC_wasm32_wasip1=%q\n' "$DEST/$sdk/bin/clang"
printf 'export AR_wasm32_wasip1=%q\n' "$DEST/$sdk/bin/llvm-ar"
printf 'export CFLAGS_wasm32_wasip1=%q\n' "--target=wasm32-wasip1 $wasi_sysroot_flag"
printf 'export CC_wasm32_wasip3=%q\n' "$DEST/$sdk/bin/clang"
printf 'export AR_wasm32_wasip3=%q\n' "$DEST/$sdk/bin/llvm-ar"
printf 'export CFLAGS_wasm32_wasip3=%q\n' "--target=wasm32-wasip3 $wasi_sysroot_flag"
printf 'export CC_SHELL_ESCAPED_FLAGS=1\n'
