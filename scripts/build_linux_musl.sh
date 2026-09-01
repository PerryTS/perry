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

# `libsqlite3-sys` (via rusqlite's `session` feature -> `buildtime_bindgen`)
# runs bindgen in its BUILD SCRIPT, and bindgen dlopen()s libclang. Cargo
# compiles build scripts for the HOST, and the host here is musl, whose
# default `crt-static` produces a fully static binary -- a static binary
# cannot dlopen anything, so the build dies with:
#
#   Unable to find libclang: "the `libclang` shared library at
#   /usr/lib/llvm22/lib/libclang.so.22.1.8 could not be opened:
#   Dynamic loading not supported"
#
# Installing libclang is necessary but NOT sufficient (that was #9357) --
# the build script itself has to be dynamically linked. `bindgen/static`
# is not an option: libsqlite3-sys pins `features = ["runtime"]` with
# default-features off, and clang-sys panics when `runtime` and `static`
# are both on.
#
# So: relax crt-static for HOST artifacts only, leaving the musl TARGET
# fully static as shipped. Written into the container's CARGO_HOME rather
# than the repo's .cargo/config.toml, because `target-applies-to-host`
# errors without its -Z flag and would break every non-musl platform.
cargo_home="${CARGO_HOME:-/opt/cargo}"
mkdir -p "$cargo_home"
cat >> "$cargo_home/config.toml" <<'CARGOCFG'

[unstable]
host-config = true
target-applies-to-host = true

[build]
target-applies-to-host = false

[host]
rustflags = ["-C", "target-feature=-crt-static"]
CARGOCFG
echo "[build_linux_musl] host build scripts: crt-static relaxed (bindgen needs dlopen)"

cargo build --profile dist --target "$target" -p perry

# The `[host]` crt-static relaxation above must NOT leak into the TARGET:
# a musl release that is dynamically linked defeats the whole point (it is
# the fully-static fallback for Alpine/distroless and old-glibc hosts).
# Assert it, rather than trusting that cargo scoped the flag correctly --
# an unverified assumption here ships a broken artifact that only fails on
# a user's machine.
perry_bin="$target_dir/$target/dist/perry"
if [ -x "$perry_bin" ]; then
  if file "$perry_bin" 2>/dev/null | grep -qi "dynamically linked"; then
    echo "musl perry is DYNAMICALLY linked -- the host crt-static relaxation leaked into the target" >&2
    file "$perry_bin" >&2
    exit 1
  fi
  echo "[build_linux_musl] verified static: $(file "$perry_bin" 2>/dev/null | cut -c1-110)"
else
  echo "expected a built perry at $perry_bin; cannot verify staticness" >&2
  exit 1
fi
cargo build --profile dist --target "$target" -p perry-runtime -p perry-runtime-static
cargo build --profile dist --target "$target" -p perry-stdlib -p perry-stdlib-static

# Ship the panic=abort runtime variant from the same sysroot.
CARGO_TARGET_DIR="$abort_target_dir" CARGO_PROFILE_DIST_PANIC=abort \
  cargo build --profile dist --target "$target" -p perry-runtime -p perry-runtime-static
cp "$abort_target_dir/$target/dist/libperry_runtime.a" \
   "$target_dir/$target/dist/libperry_runtime_abort.a"
