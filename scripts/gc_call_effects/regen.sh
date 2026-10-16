#!/usr/bin/env bash
# Regenerate (or --check) one target's committed GC call-effects table from
# freshly built runtime archives. The build command per target is FIXED here
# and in CI (.github/workflows/test.yml, job gc-call-effects): the table is a
# function of the exact archives, so a different profile or package set is a
# different table.
#
#   scripts/gc_call_effects/regen.sh linux-x86_64            # build + regenerate
#   scripts/gc_call_effects/regen.sh macos-aarch64 --check   # build + compare
#   GC_EFFECTS_SKIP_BUILD=1 GC_EFFECTS_LIB_DIR=target/release \
#       scripts/gc_call_effects/regen.sh linux-x86_64 --check
#
# The sidecars step runs on every invocation: it is the rule check for the
# archives, and every runtime archive in the directory gets its .gc-effects file.
# --cross-check also runs the retained Python analyzer and diffs complete TSV bytes.
# windows-x86_64 cross-builds with cargo-xwin (runs on a Linux or macOS host).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
target="${1:?usage: regen.sh <linux-x86_64|macos-aarch64|windows-x86_64> [--check] [--cross-check]}"
mode=""
cross_check=""
for arg in "${@:2}"; do
  case "$arg" in
    --check) mode=--check ;;
    --cross-check) cross_check=1 ;;
    *) echo "regen.sh: unknown option $arg" >&2; exit 2 ;;
  esac
done
unset RUSTFLAGS
case "$target" in
  linux-x86_64)
    # Same command as test.yml's gap-suite-build, whose archives the CI leg reuses.
    build=(cargo build --release -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static)
    libdir="target/release"; libs=(libperry_runtime.a libperry_stdlib.a) ;;
  macos-aarch64)
    build=(cargo build --release -p perry-runtime-static -p perry-stdlib-static)
    libdir="target/release"; libs=(libperry_runtime.a libperry_stdlib.a) ;;
  windows-x86_64)
    build=(cargo xwin build --release --target x86_64-pc-windows-msvc -p perry-runtime-static -p perry-stdlib-static)
    libdir="target/x86_64-pc-windows-msvc/release"; libs=(perry_runtime.lib perry_stdlib.lib) ;;
  *) echo "regen.sh: unknown target $target" >&2; exit 2 ;;
esac
libdir="${GC_EFFECTS_LIB_DIR:-$libdir}"
if [[ -z "${GC_EFFECTS_SKIP_BUILD:-}" ]]; then
  "${build[@]}"
fi
paths=()
for l in "${libs[@]}"; do paths+=("$libdir/$l"); done
table="crates/perry-codegen/src/gc_effects/$target.tsv"
tool=(cargo run --quiet --release -p perry-gc-effects --)
if [[ -n "${GC_EFFECTS_BIN:-}" ]]; then tool=("$GC_EFFECTS_BIN"); fi
# Every recipe that builds the runtime archives also writes their sidecars
# (<archive>.gc-effects), over the bytes just built.
"${tool[@]}" sidecars "$libdir"
if [[ -n "$cross_check" ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  "${tool[@]}" generate --target "$target" --out "$tmp/rust.tsv" "${paths[@]}"
  python3 scripts/gc_call_effects/callgraph.py generate --target "$target" --out "$tmp/python.tsv" "${paths[@]}"
  diff -u "$tmp/python.tsv" "$tmp/rust.tsv"
  echo "[gc-call-effects] Rust/Python cross-check: byte-identical" >&2
fi
if [[ "$mode" == "--check" ]]; then
  fresh=()
  if [[ -n "${GC_EFFECTS_FRESH_OUT:-}" ]]; then fresh=(--write-fresh "$GC_EFFECTS_FRESH_OUT"); fi
  # GC_EFFECTS_ALLOW_SAFE_DRIFT=1: fail only on UNSAFE drift (the PR tier, where
  # a PR is tested merged with a main that may have added runtime symbols).
  if [[ -n "${GC_EFFECTS_ALLOW_SAFE_DRIFT:-}" ]]; then fresh+=(--allow-safe-drift); fi
  "${tool[@]}" check --target "$target" --table "$table" \
    ${fresh[@]+"${fresh[@]}"} "${paths[@]}"
else
  "${tool[@]}" generate --target "$target" --out "$table" "${paths[@]}"
fi
