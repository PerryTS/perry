#!/usr/bin/env bash
# Regenerate the three tracked `.ll` corpora the in-process LLVM dialect reader
# is unit-tested against (`crates/perry-codegen/src/dialect/tests.rs`).
#
# WHY THIS EXISTS (#7982). The corpora are a SNAPSHOT of what codegen emitted
# when they were last refreshed. `corpus_spike ... ok` proves the tests RAN; it
# does not prove they test today's IR. Between 2026-08-03 and 2026-08-11 the
# snapshot went 151 codegen commits stale, RS4GC learned to emit
# `ptr addrspace(1)` (497 occurrences in the spike module alone), and the unit
# gate stayed green through every one of the three `main` failures the missing
# support caused. That is CLAUDE.md's fourth way a gate cannot fail — the job
# is genuinely green, its subject never ran — occurring INSIDE the liveness
# assert written to prevent it.
#
# The same thing had already happened once, nine days earlier: #7310 refreshed
# these files because they still carried setjmp calls from before #7305. A
# fixture that must be refreshed by hand will go stale again, so this script is
# the reproducible half and `scripts/check_llvm_corpus_currency.py` (in `lint`)
# is the alarm.
#
# Usage:
#   scripts/refresh_llvm_inprocess_corpora.sh [--check]
#
#   (no args)  regenerate the three corpora in place
#   --check    regenerate into a temp dir and diff; exit 1 if they differ.
#              Only meaningful on the machine/LLVM the corpora were captured
#              on — the IR carries a target triple — so this is a LOCAL
#              pre-refresh check, not a CI gate. CI's currency alarm is the
#              form-census script, which is host-independent.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$PWD"
OUT="$REPO/experiments/llvm-inprocess-spike"
PROFILE="${PERRY_CORPUS_PROFILE:-perry-dev}"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
BIN="$TARGET_DIR/$PROFILE/perry"

MODE="write"
[ "${1:-}" = "--check" ] && MODE="check"

if [ ! -x "$BIN" ]; then
  echo "error: $BIN not found." >&2
  echo "Build it first (the runtime/stdlib STATIC wrappers too, or the link" >&2
  echo "step will pick up a stale archive and you will regenerate IR from a" >&2
  echo "compiler you did not just build):" >&2
  echo "  cargo build --profile $PROFILE -p perry -p perry-runtime-static -p perry-stdlib-static" >&2
  exit 1
fi

export PERRY_RUNTIME_DIR="$TARGET_DIR/$PROFILE"
# Auto-optimize relinks against a feature-stripped runtime, which changes the
# emitted IR for reasons that have nothing to do with this snapshot.
export PERRY_NO_AUTO_OPTIMIZE=1

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# name : source : extra env  (the batch kernel is the multi-unit corpus, so it
# must be captured under the same split the CI diff arm exercises)
emit() {
  local name="$1" src="$2" units="${3:-}"
  local trace="$WORK/$name.trace"
  rm -rf "$trace"
  ( cd "$WORK" && rm -rf .perry-trace \
    && env ${units:+PERRY_CODEGEN_UNITS=$units} "$BIN" compile "$REPO/$src" \
         --trace llvm -o "$WORK/$name.bin" >/dev/null )
  local produced
  produced="$(find "$WORK/.perry-trace/llvm" -name '*.ll' | head -1)"
  if [ -z "$produced" ]; then
    echo "error: no .ll emitted for $src" >&2
    exit 1
  fi
  cp "$produced" "$WORK/$name.ll"
  echo "$WORK/$name.ll"
}

declare -a NAMES=(spike_text batch_kernel eh_text)
declare -a SRCS=(
  "experiments/llvm-inprocess-spike/spike.ts"
  "benchmarks/app-patterns/kernels/batch.ts"
  "test-files/test_gap_7302_invoke_eh_paths.ts"
)

rc=0
for i in "${!NAMES[@]}"; do
  name="${NAMES[$i]}"
  fresh="$(emit "$name" "${SRCS[$i]}")"
  tracked="$OUT/$name.ll"
  if [ "$MODE" = "check" ]; then
    if cmp -s "$fresh" "$tracked"; then
      echo "ok:    $name.ll is current"
    else
      echo "STALE: $name.ll differs from what this compiler emits" >&2
      diff -u "$tracked" "$fresh" | head -40 >&2 || true
      rc=1
    fi
  else
    cp "$fresh" "$tracked"
    echo "wrote: $name.ll ($(grep -c '^define' "$tracked") defines, \
$(grep -c 'addrspace(1)' "$tracked" || true) addrspace(1) sites)"
  fi
done

exit "$rc"
