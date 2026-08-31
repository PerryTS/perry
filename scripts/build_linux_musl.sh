#!/usr/bin/env bash
# Build the Linux musl release payload inside a musl-native container.
#
# perry links libLLVM in-process (default since 2026-08-17), so a musl target
# needs a musl-built LLVM. The Ubuntu runners only carry apt.llvm.org's glibc
# build; linking that into a static musl binary dies at the end with
# `ld-linux-x86-64.so.2: DSO missing from command line`. LLVM publishes no musl
# binaries, so the build runs inside Alpine, which packages LLVM 22.1.8.
#
# GTK4 is deliberately absent, matching the old-glibc leg: the UI archive is
# added by the host job afterwards.

set -euo pipefail

target="${1:?usage: build_linux_musl.sh <rust-target>}"
target_dir="${CARGO_TARGET_DIR:-target}"
abort_target_dir="${PERRY_ABORT_TARGET_DIR:-target-abort}"

case "$target" in
  x86_64-unknown-linux-musl)  expected_machine=x86_64 ;;
  aarch64-unknown-linux-musl) expected_machine=aarch64 ;;
  *)
    echo "unsupported musl release target: $target" >&2
    exit 2
    ;;
esac

machine=$(uname -m)
if [ "$machine" != "$expected_machine" ]; then
  echo "container architecture is $machine; $target needs $expected_machine" >&2
  exit 1
fi

# Refuse a glibc sysroot outright: the whole point of this container is that
# the LLVM being linked is musl-built. Getting this wrong reintroduces the
# exact DSO-missing failure this script exists to prevent.
if ! ldd /bin/sh 2>&1 | grep -qi musl; then
  echo "this script must run in a musl sysroot; /bin/sh is not musl-linked" >&2
  exit 1
fi

: "${LLVM_SYS_221_PREFIX:=/usr/lib/llvm22}"
export LLVM_SYS_221_PREFIX
"$LLVM_SYS_221_PREFIX/bin/llvm-config" --version | grep -q '^22\.' || {
  echo "musl image does not provide LLVM 22 under $LLVM_SYS_221_PREFIX" >&2
  exit 1
}

export PATH="${CARGO_HOME:-/opt/cargo}/bin:$PATH"
command -v rustup >/dev/null || {
  echo "rustup not found on PATH ($PATH)" >&2
  exit 1
}
rustup target add "$target"

cargo build --profile dist --target "$target" -p perry
cargo build --profile dist --target "$target" -p perry-runtime -p perry-runtime-static
cargo build --profile dist --target "$target" -p perry-stdlib -p perry-stdlib-static

# Ship the panic=abort runtime variant from the same sysroot.
CARGO_TARGET_DIR="$abort_target_dir" CARGO_PROFILE_DIST_PANIC=abort \
  cargo build --profile dist --target "$target" -p perry-runtime -p perry-runtime-static
cp "$abort_target_dir/$target/dist/libperry_runtime.a" \
   "$target_dir/$target/dist/libperry_runtime_abort.a"
