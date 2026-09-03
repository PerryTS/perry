#!/usr/bin/env bash
# Build the Linux musl release payload from a glibc host.
#
# perry links libLLVM in-process (default since 2026-08-17), so a musl target
# needs a musl-built LLVM. Cargo build scripts must remain glibc-hosted, though:
# rusqlite's session feature runs bindgen, and a native musl Rust host compiles
# that build script crt-static so it cannot dlopen libclang (#9382). The image
# therefore harvests LLVM 22 and its static libraries from Alpine, then mounts
# the runner's GNU-host Rust toolchain into a glibc final stage and
# cross-compiles the release target.
#
# GTK4 is deliberately absent, matching the old-glibc leg: the UI archive is
# added by the host job afterwards.

set -euo pipefail

target="${1:?usage: build_linux_musl.sh <rust-target>}"
target_dir="${CARGO_TARGET_DIR:-target}"
abort_target_dir="${PERRY_ABORT_TARGET_DIR:-target-abort}"

case "$target" in
  x86_64-unknown-linux-musl)
    expected_machine=x86_64
    rustflags="-C force-unwind-tables=yes -C force-frame-pointers=yes -C target-feature=+crt-static -L native=/opt/perry-musl/lib"
    export CC_x86_64_unknown_linux_musl=musl-gcc
    export AR_x86_64_unknown_linux_musl=ar
    ;;
  aarch64-unknown-linux-musl)
    expected_machine=aarch64
    rustflags="-C force-unwind-tables=yes -C target-feature=+crt-static -L native=/opt/perry-musl/lib"
    export CC_aarch64_unknown_linux_musl=musl-gcc
    export AR_aarch64_unknown_linux_musl=ar
    ;;
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

# Bindgen's build script must be dynamically linked so it can load libclang.
# Refuse a native-musl host: that is the exact configuration from #9382.
if ! getconf GNU_LIBC_VERSION 2>/dev/null | grep -q '^glibc '; then
  echo "this script needs a glibc host so bindgen can dlopen libclang" >&2
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

rust_host=$(rustc -vV | sed -n 's/^host: //p')
expected_rust_host="$expected_machine-unknown-linux-gnu"
if [ "$rust_host" != "$expected_rust_host" ]; then
  echo "Rust host is $rust_host; $target needs native host $expected_rust_host" >&2
  exit 1
fi

# GNU ld's LTO plugin cannot consume every bitcode archive in this workspace
# (the failure depends on which C dependency was cached). rust-lld avoids that
# host-plugin coupling while musl-gcc remains cc-rs's C compiler above.
rust_sysroot=$(rustc --print sysroot)
rust_lld="$rust_sysroot/lib/rustlib/$rust_host/bin/rust-lld"
if [ ! -x "$rust_lld" ]; then
  echo "rust-lld not found at $rust_lld" >&2
  exit 1
fi

case "$target" in
  x86_64-unknown-linux-musl)
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$rust_lld"
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="$rustflags"
    ;;
  aarch64-unknown-linux-musl)
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$rust_lld"
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="$rustflags"
    ;;
esac

test -s /opt/perry-musl/lib/libstdc++.a || {
  echo "musl target libraries are missing from /opt/perry-musl/lib" >&2
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

# A dynamically linked binary can run on the glibc build host and still fail
# immediately for users on Alpine. Gate the artifact itself, not just the
# Cargo exit status.
compiler="$target_dir/$target/dist/perry"
# `file` 5.39 (Debian 11) calls static PIE "dynamically linked" merely because
# it has a PT_DYNAMIC relocation table. Log its summary, but use the ELF
# interpreter and dependency tables below as the portable pass/fail criteria.
file "$compiler"
if readelf -l "$compiler" | grep -q ' INTERP '; then
  echo "musl compiler unexpectedly contains a program interpreter" >&2
  exit 1
fi
if readelf -d "$compiler" | grep -q '(NEEDED)'; then
  echo "musl compiler unexpectedly contains dynamic dependencies" >&2
  readelf -d "$compiler" >&2
  exit 1
fi
"$compiler" --version
