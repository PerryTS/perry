#!/usr/bin/env bash
#
# Emit the LLVM IR corpus that scripts/gc_root_dominance_check.py gates on.
#
# This lives in a script rather than inline in the workflow so that the corpus
# CI checks and the corpus you check locally are the same corpus. The previous
# arrangement had the source list, the env knobs and the failure budget spelled
# out only in gc-root-dominance.yml, which made "reproduce the CI failure"
# mean "reread the YAML and retype it" -- and a retyped PERRY_GC_MOVING_LOOP_POLLS
# that gets dropped produces IR in which the bug is not expressible at all.
#
#   ./scripts/gc_root_dominance_corpus.sh [OUTDIR]
#
# Requires target/release/perry plus the runtime archives (see the workflow, or
# `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`).
#
# Exit status is non-zero if fewer than MIN_COMPILED sources produced IR. A
# source that stops compiling is allowed -- test-files/ churns, and this gate is
# not the gap suite -- but the corpus is NOT allowed to quietly shrink to
# nothing, because "0 violations over 3 files" and "0 violations over 300 files"
# print the same verdict and mean opposite things.

set -euo pipefail

OUTDIR="${1:-ir-corpus}"
PERRY_BIN="${PERRY_BIN:-target/release/perry}"

# Sources are chosen for the LOWERINGS they exercise, not for coverage of the
# language. Every one of #7154/#7184/#7192/#7206 was in one of these paths:
#
#   gc/repsel  - the rooting machinery itself, and representation selection
#                (a value that changes representation changes who must root it)
#   class      - method receivers, `super()`, static initialisers. #7206's
#                receiver-across-the-argument-list bug lives here.
#   new        - inline-constructor `this_slot`, the still-open alloca case
#   object/    - object and array literals: long runs of element stores, each
#     array      one an allocation the accumulating literal must survive
#   computed/  - computed reads. #7206's other bug is the base held in a
#     index      register across the key expression's own allocations
#   prop       - property stores
#   static     - static field initialisers, which run in a synthetic frame
#   closure    - captures, the js_closure_callN family
#   dynamic    - dynamic dispatch, where the receiver's type is not proven
#   spread     - spread/rest, which allocate per element
#   map/set    - collection literals
#
# Keep this list in sync with MIN_COMPILED below when you add a prefix.
PATTERNS=(
  'test_gap_gc_*.ts'
  'test_gap_repsel*.ts'
  'test_gap_class*.ts'
  'test_gap_new*.ts'
  'test_gap_object*.ts'
  'test_gap_array*.ts'
  'test_gap_computed*.ts'
  'test_gap_prop*.ts'
  'test_gap_static*.ts'
  'test_gap_closure*.ts'
  'test_gap_dynamic*.ts'
  'test_gap_map*.ts'
  'test_gap_set*.ts'
)

# The floor, not a target. Raise it when the corpus grows; never lower it to
# make a run pass -- a shrinking corpus is the finding, not the obstacle.
MIN_COMPILED="${MIN_COMPILED:-90}"

if [ ! -x "$PERRY_BIN" ]; then
  echo "error: $PERRY_BIN not found or not executable." >&2
  echo "  cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static" >&2
  exit 2
fi

shopt -s nullglob
sources=()
for pat in "${PATTERNS[@]}"; do
  # shellcheck disable=SC2206
  matches=(test-files/$pat)
  if [ "${#matches[@]}" -eq 0 ]; then
    # Loud, because a stale pattern is how a corpus silently loses a whole
    # lowering: the run stays green and nobody re-reads the glob list.
    echo "::error::pattern '$pat' matched nothing; it is stale. Remove it or fix it." >&2
    exit 2
  fi
  sources+=("${matches[@]}")
done

if [ "${#sources[@]}" -eq 0 ]; then
  echo "::error::no corpus sources matched any pattern; the glob list is stale" >&2
  exit 2
fi

rm -rf "$OUTDIR"
mkdir -p "$OUTDIR"

compiled=0
skipped=0
skipped_names=()
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

for src in "${sources[@]}"; do
  name="$(basename "$src" .ts)"
  rm -rf .perry-trace/llvm
  # PERRY_GC_MOVING_LOOP_POLLS=1 is what puts `js_gc_loop_safepoint` in the IR,
  # which is what the MOVING classification keys on. It is off by default
  # (#7161), so without it this corpus cannot express the bug at all.
  # PERRY_INLINE_SHADOW_SLOT=0 makes every root store the @js_shadow_slot_bind
  # call form; the #7088 inline diamond is equivalent but harder to anchor on.
  if ! env PERRY_GC_MOVING_LOOP_POLLS=1 \
           PERRY_INLINE_SHADOW_SLOT=0 \
           PERRY_NO_AUTO_OPTIMIZE=1 \
       "$PERRY_BIN" compile "$src" -o "$scratch/$name" --trace llvm \
       >/dev/null 2>&1; then
    skipped=$((skipped + 1))
    skipped_names+=("$name")
    continue
  fi
  emitted=0
  for ll in .perry-trace/llvm/*.ll; do
    cp "$ll" "$OUTDIR/${name}__$(basename "$ll")"
    emitted=1
  done
  if [ "$emitted" -eq 1 ]; then
    compiled=$((compiled + 1))
  else
    # Compiled but emitted no IR: --trace llvm is wired to codegen, so this
    # means codegen did not run (a fully cached build). Counting it as a
    # success would let a cache hit stand in for a check.
    skipped=$((skipped + 1))
    skipped_names+=("$name (no .ll emitted)")
  fi
done

files="$(find "$OUTDIR" -name '*.ll' | wc -l | tr -d ' ')"
echo "corpus: $compiled/${#sources[@]} sources compiled, $skipped skipped, $files .ll files"
if [ "$skipped" -gt 0 ]; then
  printf '  skipped: %s\n' "${skipped_names[*]}"
fi

if [ "$compiled" -lt "$MIN_COMPILED" ]; then
  echo "::error::only $compiled sources compiled, need at least $MIN_COMPILED." >&2
  echo "The corpus shrank. Either fix the sources that stopped compiling, or -- if" >&2
  echo "they were removed on purpose -- adjust PATTERNS and MIN_COMPILED together" >&2
  echo "in this file, in the same PR, so the reduction is reviewable." >&2
  exit 1
fi
