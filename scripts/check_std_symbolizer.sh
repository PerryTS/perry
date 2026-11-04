#!/usr/bin/env bash
# Fails if a runtime archive or a linked program defines std's DWARF
# symbolizer (std::backtrace_rs::symbolize::gimli: gimli, addr2line,
# miniz_oxide, object; the code std's `backtrace` feature adds).
#
# Every cargo build of this workspace compiles std from rust-src without its
# `backtrace` feature (.cargo/config.toml [unstable]). The prebuilt std has
# the feature, and its default panic hook links the symbolizer (gimli,
# addr2line, miniz_oxide, object, rustc-demangle) into every program.
# This check catches any build path that comes back to the prebuilt std: a
# cargo invocation outside the workspace config, `--config` overrides, a
# toolchain without rust-src that someone works around.
#
# Windows MSVC: std's symbolizer there is dbghelp (a system DLL), which std
# compiles with or without the feature, in the prebuilt std and here alike;
# the feature changes only whether a panic reaches it (here it cannot:
# get_backtrace_style() is the constant None). So on MSVC this check reads
# the archive (fail-closed) but has nothing to tell apart.
#
# Only std's own namespace is matched. perry-runtime uses gimli (eh_walker's
# CFI reader), miniz_oxide and rustc-demangle itself, and wasmtime brings
# addr2line; those names are not std's symbolizer.
#
# Usage: check_std_symbolizer.sh <archive | .lib | linked binary>...
set -euo pipefail
if [ "$#" -lt 1 ]; then
  echo "usage: $0 <archive-or-binary>..." >&2
  exit 2
fi
# llvm-nm, looked up as the other archive gates do: rustup's llvm-tools nm
# (rustc's own LLVM), then $LLVM_SYS_221_PREFIX (the CI LLVM; a Windows path
# under MSYS/Git Bash, converted with cygpath), then PATH. A candidate counts
# only if it is LLVM 22 or newer: an older nm cannot read the archive members
# and would make every listing empty. No such nm: fail closed.
llvm_major() {
  "$1" --version 2>/dev/null | sed -n 's/.*LLVM version \([0-9][0-9]*\).*/\1/p' | head -1
}
cands=()
if command -v rustc >/dev/null 2>&1; then
  sysroot=$(rustc --print sysroot 2>/dev/null || true)
  host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
  if [ -n "$sysroot" ] && [ -n "$host" ]; then
    cands+=("$sysroot/lib/rustlib/$host/bin/llvm-nm")
  fi
fi
if [ -n "${LLVM_SYS_221_PREFIX:-}" ]; then
  prefix=$LLVM_SYS_221_PREFIX
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) prefix=$(cygpath -u "$prefix") ;;
  esac
  cands+=("$prefix/bin/llvm-nm")
fi
cands+=(llvm-nm-22 llvm-nm)
NM=""
for cand in "${cands[@]}"; do
  command -v "$cand" >/dev/null 2>&1 || continue
  major=$(llvm_major "$cand")
  if [ -n "$major" ] && [ "$major" -ge 22 ]; then NM=$cand; break; fi
done
if [ -z "$NM" ]; then
  echo "::error::check_std_symbolizer: no llvm-nm of LLVM 22 or newer (tried: ${cands[*]}); install rustup llvm-tools or set LLVM_SYS_221_PREFIX" >&2
  exit 2
fi
PATTERN='std::backtrace_rs::symbolize::gimli'
status=0
for f in "$@"; do
  if [ ! -f "$f" ]; then
    echo "::error::check_std_symbolizer: $f does not exist" >&2
    status=1
    continue
  fi
  listing=$("$NM" -C --defined-only "$f" 2>/dev/null | tr -d '\r' || true)
  # A listing that names no std symbol at all means nm could not read the
  # members: fail instead of passing silently.
  if ! grep -q 'std::' <<<"$listing"; then
    echo "::error::check_std_symbolizer: $NM read no std symbols from $f" >&2
    status=1
    continue
  fi
  hits=$(grep -E "$PATTERN" <<<"$listing" || true)
  if [ -n "$hits" ]; then
    echo "::error::$f defines std's symbolizer ($(wc -l <<<"$hits") symbols): std was not built from rust-src without its backtrace feature" >&2
    head -5 <<<"$hits" >&2
    status=1
  else
    echo "ok: $f has no std symbolizer"
  fi
done
exit "$status"
