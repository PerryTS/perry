#!/usr/bin/env bash
#
# Emit the DEPENDENCY-SCALE LLVM IR corpus for
# scripts/gc_root_dominance_check.py.
#
#   ./scripts/gc_root_dominance_dep_corpus.sh [OUTDIR]
#
# The sibling script, gc_root_dominance_corpus.sh, compiles ~99 hand-written
# `test-files/` sources. That corpus reads ZERO in both modes the CI gate runs
# — and it read zero while twenty lines of stock `zod` faulted deterministically
# under the from-space protector (#7280). Twenty-five curated files passing
# while a real dependency fails is not noise, it is the corpus measuring the
# wrong population:
#
#   curated, --stale-registers --moving-only:
#     property-GET helper windows, ToPrimitive/js_number_coerce, js_closure_callN
#   dependency-scale, the same command:
#     js_array_alloc -> js_array_spread_append/concat,
#     js_box_get_bits -> js_closure_callN,
#     js_object_alloc -> js_object_set_field_by_name
#
# A hand-written test allocates a couple of objects and calls a couple of
# helpers. A library spreads arrays into arrays, boxes every mutable capture
# because its closures outlive their frames, and builds objects field by field
# out of data. The rooting hazards live in the SHAPES, so a corpus without the
# shapes cannot express them however many files it has.
#
# The dependency is `zod`, this repo's own `package.json` devDependency, pinned
# by `package-lock.json` and governed by the same soak window as everything
# else in that file. It is imported BY SOURCE PATH
# (`node_modules/zod/src/index.js`) rather than by bare specifier, because that
# is what makes its modules NATIVE — a bare import resolves to the published
# bundle and can fall back to V8, which emits no IR to check.
#
# Exit status is non-zero if the corpus is thinner than MIN_MODULES. "0
# violations over 3 modules" and "0 violations over 90 modules" print the same
# verdict and mean opposite things, and this corpus has one more way to go thin
# than the curated one does: a checkout with no `node_modules/`.

set -euo pipefail

OUTDIR="${1:-ir-corpus-dep}"
PERRY_BIN="${PERRY_BIN:-target/release/perry}"
ENTRY="${ENTRY:-test-files/gc-dep-corpus/main.ts}"

# The floor, not a target. `zod@4` compiles to ~90 native modules here; the
# floor sits below that with room for the dependency's own churn and well above
# "something compiled". Raise it when the dependency grows; never lower it to
# make a run pass.
MIN_MODULES="${MIN_MODULES:-60}"

if [ ! -x "$PERRY_BIN" ]; then
  echo "error: $PERRY_BIN not found or not executable." >&2
  echo "  cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static" >&2
  exit 2
fi

if [ ! -f node_modules/zod/src/index.ts ]; then
  echo "::error::node_modules/zod/src/index.ts is missing." >&2
  echo "This corpus is generated FROM a real npm dependency, so it needs the" >&2
  echo "dependency. Run:" >&2
  echo "  npm ci --ignore-scripts" >&2
  echo "Refusing to emit a corpus without it: a compile that silently drops the" >&2
  echo "library still produces .ll files, and a thin corpus reporting zero is" >&2
  echo "exactly the false green this whole gate exists to prevent." >&2
  exit 2
fi

if [ ! -f "$ENTRY" ]; then
  echo "::error::corpus entry point $ENTRY is missing" >&2
  exit 2
fi

rm -rf "$OUTDIR" .perry-trace/llvm
mkdir -p "$OUTDIR"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# Same two env knobs as the curated corpus, for the same two reasons.
# PERRY_GC_MOVING_LOOP_POLLS=1 is what puts `js_gc_loop_safepoint` in the IR
# (it is off by default since #7161, so without it the corpus cannot express a
# back-edge collection at all). PERRY_INLINE_SHADOW_SLOT=0 makes every root
# store the @js_shadow_slot_bind call form the checker anchors on.
if ! env PERRY_GC_MOVING_LOOP_POLLS=1 \
         PERRY_INLINE_SHADOW_SLOT=0 \
         PERRY_NO_AUTO_OPTIMIZE=1 \
     "$PERRY_BIN" compile "$ENTRY" -o "$scratch/dep-corpus" --trace llvm \
     >"$scratch/compile.log" 2>&1; then
  echo "::error::$ENTRY failed to compile; the corpus cannot be emitted" >&2
  tail -40 "$scratch/compile.log" >&2
  exit 1
fi

modules=0
for ll in .perry-trace/llvm/*.ll; do
  [ -e "$ll" ] || break
  cp "$ll" "$OUTDIR/dep__$(basename "$ll")"
  modules=$((modules + 1))
done

bytes="$(find "$OUTDIR" -name '*.ll' -exec cat {} + | wc -c | tr -d ' ')"
echo "dep corpus: $modules module(s), $((bytes / 1024)) KiB of IR in $OUTDIR"

if [ "$modules" -lt "$MIN_MODULES" ]; then
  echo "::error::only $modules module(s) emitted IR, need at least $MIN_MODULES." >&2
  echo "Either the dependency stopped compiling natively (which is the finding)," >&2
  echo "or --trace llvm did not run because the build was fully cached." >&2
  exit 1
fi

# ★ Every source in the corpus directory must have contributed a module.
#
# There is ONE entry point and the rest of the directory reaches the compiler
# only by being imported from it. A file nobody imports is compiled by nothing
# and checked by nothing — a dark test with a corpus file's name on it (#7278),
# and one this corpus cannot detect by counting, because 90 modules of `zod`
# swamp any floor a missing 40-line source would cross.
#
# So the registry is the import graph and this is the check on it: the module
# name perry derives from a path is the path with every non-alphanumeric
# character replaced by `_`, so each source names exactly one expected `.ll`.
dark=()
for src in test-files/gc-dep-corpus/*.ts; do
  sanitized="$(printf '%s' "$src" | tr -c 'A-Za-z0-9' '_')"
  if [ ! -f "$OUTDIR/dep__${sanitized}.ll" ]; then
    dark+=("$src")
  fi
done
if [ "${#dark[@]}" -gt 0 ]; then
  echo "::error::these corpus sources produced no IR — nothing imports them:" >&2
  printf '  %s\n' "${dark[@]}" >&2
  echo "Import them from test-files/gc-dep-corpus/main.ts, or delete them." >&2
  echo "An unimported corpus source is not a weak test, it is no test at all." >&2
  exit 1
fi
