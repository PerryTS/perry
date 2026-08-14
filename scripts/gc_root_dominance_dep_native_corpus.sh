#!/usr/bin/env bash
# Emit the DEPENDENCY-SCALE corpus under the NATIVE (statepoint) lowering.
#
# WHY THIS EXISTS
#
# gc-root-dominance.yml has four corpus/lowering combinations, and one of the
# four is missing:
#
#                       shadow (PERRY_RS4GC=0)   native (statepoints, SHIPS)
#   curated ~124 files  gated                    gated, --max-unrooted 0
#   dependency (zod)    gated, --max-stale 118   THIS SCRIPT (was missing)
#
# #7280's own finding was that the curated corpus is the wrong POPULATION --
# 25 curated files pass while 20 lines of stock zod fault. #7452's finding was
# that the shadow lowering is the wrong LOWERING -- statepoints became the
# default in #7370 and the shadow corpus contains zero of the shipping root
# form. The cell where both corrections meet is the one nobody has ever run,
# and it is exactly the configuration #7803 fails in: the zod corpus, compiled
# the way the failing binary was compiled. First measurement, at #7803: 66
# `unrooted` hazards where the curated corpus in the identical mode is gated at
# ZERO. 40 of them were the callee-outlives-arguments defect in three call
# lowering arms; the residual is a budget that can only go down.
set -euo pipefail
OUTDIR="${1:-ir-corpus-dep-native}"
PERRY_BIN="${PERRY_BIN:-target/release/perry}"
# `env` execs the binary directly, so a relative path is resolved against the
# CWD at exec time rather than against the repo root. Pin it now.
case "$PERRY_BIN" in /*) ;; *) PERRY_BIN="$PWD/$PERRY_BIN" ;; esac
[ -x "$PERRY_BIN" ] || { echo "::error::$PERRY_BIN not found or not executable" >&2; exit 2; }
ENTRY="${ENTRY:-test-files/gc-dep-corpus/main.ts}"

# Single-sourced from the Rust const, never retyped (same reason the curated
# script gives: a fourth copy is how the pass string drifts from production).
# Join continuation lines up to the `;` first: rustfmt wraps the initializer
# when the line grows (#8068 did), and a single-line match then reads nothing.
PASSES="$(awk '/const STATEPOINT_REWRITE_PASSES: &str/ {
    buf = $0
    while (buf !~ /;[[:space:]]*$/ && (getline line) > 0) buf = buf " " line
    if (match(buf, /"[^"]*"/)) print substr(buf, RSTART + 1, RLENGTH - 2)
    exit
  }' crates/perry-codegen/src/inprocess.rs)"
[ -n "$PASSES" ] || { echo "could not read STATEPOINT_REWRITE_PASSES" >&2; exit 2; }
OPT_BIN="${PERRY_LLVM_OPT:-/opt/homebrew/opt/llvm/bin/opt}"
if [ ! -x "$OPT_BIN" ]; then
  for c in "${LLVM_SYS_221_PREFIX:-}/bin/opt" /opt/homebrew/opt/llvm/bin/opt /usr/local/opt/llvm/bin/opt; do
    [ -n "$c" ] && [ -x "$c" ] && OPT_BIN="$c" && break
  done
fi
[ -x "$OPT_BIN" ] || { echo "::error::no LLVM \`opt\` found; set PERRY_LLVM_OPT" >&2; exit 2; }
if [ ! -f node_modules/zod/src/index.ts ]; then
  echo "::error::node_modules/zod/src/index.ts is missing; run npm ci --ignore-scripts" >&2
  exit 2
fi
echo "passes: $PASSES"
echo "opt:    $OPT_BIN"

rm -rf "$OUTDIR" .perry-trace/llvm
mkdir -p "$OUTDIR"
scratch="$(mktemp -d)"

env PERRY_RS4GC=1 \
    PERRY_GC_MOVING_LOOP_POLLS=1 \
    PERRY_INLINE_SHADOW_SLOT=0 \
    PERRY_NO_AUTO_OPTIMIZE=1 \
    "$PERRY_BIN" compile "$ENTRY" -o "$scratch/dep-native" --trace llvm \
    >"$scratch/compile.log" 2>&1 || { tail -40 "$scratch/compile.log" >&2; exit 1; }

mods=0; failed=0
for ll in .perry-trace/llvm/*.ll; do
  [ -e "$ll" ] || break
  out="$OUTDIR/dep__$(basename "$ll")"
  if "$OPT_BIN" -passes="$PASSES" -S "$ll" -o "$out" 2>"$scratch/opt.err"; then
    mods=$((mods + 1))
  else
    rm -f "$out"; failed=$((failed + 1))
    echo "  rewrite failed: $(basename "$ll") -- $(head -1 "$scratch/opt.err")"
  fi
done

# Subject liveness, asserted at generation time: `opt` exits 0 on a module with
# nothing to rewrite, so "no statepoints" and "clean" are indistinguishable
# downstream.
sp="$(grep -ho 'gc\.statepoint\.p0(' "$OUTDIR"/*.ll 2>/dev/null | wc -l | tr -d ' ')"
live="$(grep -ho '"gc-live"(' "$OUTDIR"/*.ll 2>/dev/null | wc -l | tr -d ' ')"
echo "dep-native corpus: $mods modules ($failed rewrite failures)"
echo "  statepoints: $sp   non-empty live bundles: $live"
[ "$sp" -gt 0 ] && [ "$live" -gt 0 ] || { echo "::error::corpus has nothing to check" >&2; exit 1; }
rm -rf "$scratch"
