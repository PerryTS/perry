#!/usr/bin/env bash
# Post-build freshness guard for runtime static libraries (#4856).
#
# A Swatinem/rust-cache restore can leave `target/` fingerprints that make
# cargo treat workspace crates as up-to-date and silently reuse a
# `libperry_runtime.a` built from older sources — v0.5.1150 shipped Apple
# cross runtimes missing `perry_macos_bundle_chdir` exactly that way, which
# broke every iOS/tvOS executable link. This script asserts that a built
# runtime archive defines the sentinel `#[no_mangle]` symbols below, so a
# stale archive fails the release instead of shipping.
#
# Usage: check_runtime_symbols.sh <libperry_runtime.a | perry_runtime.lib>...
#
# When codegen starts referencing a new unconditional runtime symbol from
# every program's `main` prelude (see perry-codegen/src/codegen/entry.rs),
# add it to SENTINELS so a stale runtime missing it is caught here, not on
# a build worker at link time.

set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <runtime-archive>..." >&2
  exit 2
fi

# Each sentinel must be defined unconditionally in perry-runtime — i.e. a
# `#[no_mangle] pub extern "C" fn` with no `#[cfg]` on the item or its
# module (a cfg-gated *body* is fine; the symbol still exists everywhere).
SENTINELS=(
  js_gc_init
  perry_macos_bundle_chdir # added by #4833; absence = pre-#4833 stale archive
)

# Tool preference: rustup's llvm-tools nm (matches rustc's LLVM, reads the
# thin-LTO bitcode members) → PATH llvm-nm → system nm. The fallbacks may
# fail to parse bitcode members, but `--print-armap` below only needs the
# archive symbol index (ranlib map), which every archiver writes as plain
# data — readable regardless of member object format.
NM=""
if command -v rustc >/dev/null 2>&1; then
  sysroot=$(rustc --print sysroot 2>/dev/null || true)
  host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
  if [ -n "$sysroot" ] && [ -n "$host" ] && [ -x "$sysroot/lib/rustlib/$host/bin/llvm-nm" ]; then
    NM="$sysroot/lib/rustlib/$host/bin/llvm-nm"
  fi
fi
if [ -z "$NM" ] && command -v llvm-nm >/dev/null 2>&1; then
  NM=llvm-nm
fi
if [ -z "$NM" ] && command -v nm >/dev/null 2>&1; then
  NM=nm
fi
if [ -z "$NM" ]; then
  echo "::warning::check_runtime_symbols: no llvm-nm/nm available — skipping symbol guard" >&2
  exit 0
fi

status=0
for lib in "$@"; do
  if [ ! -f "$lib" ]; then
    echo "::error::check_runtime_symbols: $lib does not exist" >&2
    status=1
    continue
  fi
  # `--print-armap` emits the archive symbol index ("sym in member.o") in
  # addition to per-member listings; unreadable members only lose the
  # latter. Tokenize, strip the Mach-O leading underscore, exact-match —
  # no substring false positives (`foo_js_gc_init` ≠ `js_gc_init`).
  tokens=$("$NM" --print-armap "$lib" 2>/dev/null | tr -d '\r' | tr ' \t' '\n\n' | sed 's/^_//' | sort -u || true)
  missing=0
  for sym in "${SENTINELS[@]}"; do
    if ! grep -qx "$sym" <<<"$tokens"; then
      echo "::error::$lib does not define runtime symbol '$sym' — stale cached build artifact? (#4856)" >&2
      missing=1
      status=1
    fi
  done
  if [ "$missing" -eq 0 ]; then
    echo "ok: $lib defines all ${#SENTINELS[@]} sentinel symbols"
  fi
done
exit "$status"
