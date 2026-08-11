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
#   ./scripts/gc_root_dominance_corpus.sh [OUTDIR] [--lowering shadow|native]
#
# Requires target/release/perry plus the runtime archives (see the workflow, or
# `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`).
#
# Exit status is non-zero if PATTERNS matches fewer than MIN_SOURCES files, or
# if more than MAX_SKIPPED (= 0) of them fail to produce IR. A source that stops
# compiling is NOT allowed to pass quietly: it emits no IR, and IR that was
# never emitted reads to the checker exactly like IR with no violations in it.
# Nor is the corpus allowed to shrink, because "0 violations over 3 files" and
# "0 violations over 300 files"
# print the same verdict and mean opposite things.
#
# TWO LOWERINGS, and the corpus is not the same corpus for both (#7663)
# ---------------------------------------------------------------------
#   --lowering shadow   PERRY_RS4GC=0: roots are `@js_shadow_slot_bind` calls.
#                       This is what `gc_root_dominance_check.py`'s default,
#                       `--stale-registers` and `--unrooted-allocas` modes read.
#                       Still the lowering that SHIPS on arm64_32 watchOS and
#                       ARM64 Windows -- the targets whose frames the runtime
#                       cannot walk.
#   --lowering native   PERRY_RS4GC=1: roots are `gc.statepoint` relocation
#                       bundles. The default on every other target since #7370.
#                       Read by `--statepoints`.
#
# The native arm needs one extra step, and it is the whole reason this was not a
# one-line flag. `--trace llvm` dumps what CODEGEN emitted, and codegen does not
# emit statepoints: it emits `ptr addrspace(1)` root allocas and a
# `gc "statepoint-example"` function attribute, and LLVM's
# `rewrite-statepoints-for-gc` turns those into safepoints later, inside the
# linker step (`perry-codegen/src/linker.rs`, `maybe_rs4gc_preprocess`). Measured
# on this corpus: the traced IR under the default lowering contains ZERO
# `gc.statepoint` instructions and ZERO `"gc-live"` bundles -- only the
# `declare` lines. So the native corpus is the traced IR PLUS the production
# rewrite, run here with the same pass string production uses.
#
# That pass string is single-sourced from the Rust
# (`STATEPOINT_REWRITE_PASSES`) and checked, not copied: a reproduction of a
# pipeline that has silently drifted is a corpus about nothing. See
# `rs4gc_pass_string`.

set -euo pipefail

OUTDIR="ir-corpus"
LOWERING="${LOWERING:-shadow}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --lowering)
      LOWERING="${2:-}"
      shift 2
      ;;
    --lowering=*)
      LOWERING="${1#--lowering=}"
      shift
      ;;
    -*)
      echo "error: unknown flag $1" >&2
      exit 2
      ;;
    *)
      OUTDIR="$1"
      shift
      ;;
  esac
done
case "$LOWERING" in
  shadow|native) ;;
  *)
    echo "error: --lowering must be 'shadow' or 'native' (got '$LOWERING')" >&2
    exit 2
    ;;
esac
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

# --- the two ratchets, and why a lone MIN_COMPILED could not fail -----------
#
# This used to be `MIN_COMPILED=90`, hand-synced to PATTERNS by a comment. The
# comment drifted: PATTERNS grew to discover **131** sources while the floor
# stayed at 90, so the run tolerated **41 sources failing to compile** and still
# exited 0, printing the failures to a log nobody reads. That is CLAUDE.md's
# hazard 4 in its purest form -- the gate ran, 41 of its subjects need not have.
#
# A floor expressed as an absolute count cannot track a corpus that grows. Both
# numbers below are therefore ratchets against the corpus as DISCOVERED, and
# both are checked in BOTH directions, so neither can drift above reality again:
#
#   MIN_SOURCES   how many files PATTERNS must still match. Falls only if
#                 sources were deleted or renamed -- the "corpus shrank"
#                 finding, which the old floor conflated with "sources failed
#                 to compile". Two different failures, two different messages.
#   MAX_SKIPPED   how many discovered sources may fail to compile. It is 0, and
#                 0 is the measured truth on both lowerings, not an aspiration:
#                 shadow and native each report 131/131, 0 skipped as of
#                 v0.5.1402. A skip is a finding, not a tolerance.
#
# Raise MIN_SOURCES when you add a prefix; there is nothing else to keep in
# sync, because the compile floor is now DERIVED (MIN_SOURCES - MAX_SKIPPED)
# rather than restated. Lowering either to make a run pass is the thing this
# comment exists to stop.
MIN_SOURCES="${MIN_SOURCES:-131}"
MAX_SKIPPED="${MAX_SKIPPED:-0}"

if [ ! -x "$PERRY_BIN" ]; then
  echo "error: $PERRY_BIN not found or not executable." >&2
  echo "  cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static" >&2
  exit 2
fi

# --- the native arm's two prerequisites ------------------------------------
#
# Both are resolved BEFORE the compile loop, so a missing `opt` or a drifted
# pass string fails in one second rather than after ~100 compiles.

# The pass string, read out of the Rust rather than retyped here.
#
# `STATEPOINT_REWRITE_PASSES` is a `const` in perry-codegen precisely so that
# the coverage suite and production cannot drift (`inprocess.rs` says so). This
# corpus is a third consumer, and a shell script holding a fourth copy is how
# that promise ends. Extract it, and treat "not found" as a hard error: a
# corpus generated by a pass pipeline production stopped using is a corpus
# about nothing, and it would report a serene zero.
rs4gc_pass_string() {
  local src="crates/perry-codegen/src/inprocess.rs"
  if [ ! -f "$src" ]; then
    echo "::error::$src not found; run from the repository root" >&2
    return 1
  fi
  local value
  value="$(sed -n 's/^pub(crate) const STATEPOINT_REWRITE_PASSES: &str = "\(.*\)";$/\1/p' "$src")"
  if [ -z "$value" ]; then
    echo "::error::could not read STATEPOINT_REWRITE_PASSES out of $src." >&2
    echo "The native corpus reproduces production's statepoint rewrite, and it" >&2
    echo "single-sources the pass string from that const so the two cannot" >&2
    echo "drift. If the const was renamed or reformatted, update this reader --" >&2
    echo "do NOT hardcode the string here, which is the drift this exists to" >&2
    echo "prevent." >&2
    return 1
  fi
  printf '%s\n' "$value"
}

find_opt() {
  local candidate
  for candidate in \
      "${PERRY_LLVM_OPT:-}" \
      "${LLVM_SYS_221_PREFIX:-}/bin/opt" \
      /opt/homebrew/opt/llvm/bin/opt \
      /usr/local/opt/llvm/bin/opt; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  if command -v opt >/dev/null 2>&1; then
    command -v opt
    return 0
  fi
  return 1
}

RS4GC_PASSES=""
OPT_BIN=""
if [ "$LOWERING" = "native" ]; then
  RS4GC_PASSES="$(rs4gc_pass_string)" || exit 2
  if ! OPT_BIN="$(find_opt)"; then
    echo "::error::no LLVM \`opt\` found. The native corpus needs it: codegen" >&2
    echo "emits pre-statepoint IR and \`rewrite-statepoints-for-gc\` is what" >&2
    echo "turns it into the lowering that ships. Set PERRY_LLVM_OPT or" >&2
    echo "LLVM_SYS_221_PREFIX, or install Homebrew LLVM." >&2
    exit 2
  fi
  echo "native lowering: $OPT_BIN  -passes='$RS4GC_PASSES'"
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
opt_failed=0
opt_failed_names=()
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
  # PERRY_RS4GC selects the ROOT LOWERING, and both values are spelled
  # explicitly rather than left to the default. Statepoints became the default
  # in #7370; a corpus that inherits "whatever the default is today" changes
  # subject silently the next time that flips, which is how `gc-root-dominance`
  # came to be reading a lowering that does not ship.
  #
  #   shadow (=0)  roots are `@js_shadow_slot_bind` calls. The `--min-binds`
  #                floor is about these.
  #   native (=1)  roots are `ptr addrspace(1)` values that
  #                `rewrite-statepoints-for-gc` turns into `gc.statepoint`
  #                relocation bundles below. Zero binds by construction, which
  #                is why `--statepoints` has its own floors.
  if [ "$LOWERING" = "native" ]; then rs4gc=1; else rs4gc=0; fi
  if ! env PERRY_RS4GC="$rs4gc" \
           PERRY_GC_MOVING_LOOP_POLLS=1 \
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
    out="$OUTDIR/${name}__$(basename "$ll")"
    if [ "$LOWERING" = "native" ]; then
      # The production rewrite. A module that `opt` refuses is SKIPPED rather
      # than copied through unrewritten: unrewritten IR parses fine and
      # contains no statepoints, so it would dilute the corpus with files the
      # checker reads as clean.
      if ! "$OPT_BIN" -passes="$RS4GC_PASSES" -S "$ll" -o "$out" 2>"$scratch/opt.err"; then
        rm -f "$out"
        opt_failed=$((opt_failed + 1))
        opt_failed_names+=("$(basename "$ll") -- $(head -1 "$scratch/opt.err")")
        continue
      fi
    else
      cp "$ll" "$out"
    fi
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
echo "corpus ($LOWERING): $compiled/${#sources[@]} sources compiled, $skipped skipped, $files .ll files"
if [ "$skipped" -gt 0 ]; then
  printf '  skipped: %s\n' "${skipped_names[*]}"
fi
if [ "$opt_failed" -gt 0 ]; then
  printf '  rewrite failed: %s\n' "${opt_failed_names[*]}"
fi

# ★ The native arm's SUBJECT-LIVENESS assertion, and it belongs here rather
# than in the checker.
#
# `opt` exits 0 on a module with nothing to rewrite. If codegen ever stopped
# emitting `gc "statepoint-example"` -- a one-line target-gating change --
# every module would rewrite cleanly to itself, the corpus would contain zero
# safepoints, and `--statepoints` would have nothing to look at. The checker
# has its own `--min-statepoints` floor for that, but a corpus that KNOWS it is
# empty should say so at the point it was generated, not leave the diagnosis to
# a downstream floor whose message is about a different thing.
if [ "$LOWERING" = "native" ]; then
  sp="$(grep -ho 'gc\.statepoint\.p0(' "$OUTDIR"/*.ll 2>/dev/null | wc -l | tr -d ' ')"
  live="$(grep -ho '"gc-live"(' "$OUTDIR"/*.ll 2>/dev/null | wc -l | tr -d ' ')"
  echo "  statepoints: $sp   non-empty live bundles: $live"
  if [ "$sp" -eq 0 ] || [ "$live" -eq 0 ]; then
    echo "::error::the native corpus contains $sp statepoint(s) and $live live" >&2
    echo "bundle(s). The rewrite ran and produced nothing to check. Either" >&2
    echo "codegen stopped marking functions gc \"statepoint-example\", or" >&2
    echo "PERRY_RS4GC=1 no longer selects native roots for this target." >&2
    exit 1
  fi
fi

# --- ratchet 1: did PATTERNS still find the corpus? -------------------------
#
# Separate from the skip check below on purpose. "13 sources vanished" and "13
# sources failed to compile" are different findings with different fixes, and
# the single floor this replaced reported both as the same thing.
if [ "${#sources[@]}" -lt "$MIN_SOURCES" ]; then
  echo "::error::PATTERNS matched only ${#sources[@]} sources, expected at least $MIN_SOURCES." >&2
  echo "The corpus shrank -- sources were deleted or renamed out from under it." >&2
  echo "If that was deliberate, lower MIN_SOURCES in this file in the SAME PR so" >&2
  echo "the reduction is reviewable." >&2
  exit 1
fi

# --- ratchet 2: every discovered source must compile ------------------------
#
# Two-sided. Over budget is a regression; UNDER budget means the budget is
# stale and would silently absorb the next real failure, so it fails too and
# names the number to write down. With MAX_SKIPPED=0 the second arm is
# unreachable today -- it is what keeps the number honest if anyone ever
# raises it, which is precisely how MIN_COMPILED=90 came to tolerate 41.
if [ "$skipped" -gt "$MAX_SKIPPED" ]; then
  echo "::error::$skipped of ${#sources[@]} sources failed to compile (budget: $MAX_SKIPPED)." >&2
  echo "  ${skipped_names[*]}" >&2
  echo "Fix them. Raising MAX_SKIPPED hides a compiler regression from every" >&2
  echo "downstream floor in this script -- IR that was never emitted reads as" >&2
  echo "clean to the checker." >&2
  exit 1
fi
if [ "$skipped" -lt "$MAX_SKIPPED" ]; then
  echo "::error::only $skipped sources skipped, but MAX_SKIPPED is $MAX_SKIPPED." >&2
  echo "The budget is stale and would absorb the next real failure silently." >&2
  echo "Lower MAX_SKIPPED to $skipped in this file." >&2
  exit 1
fi
