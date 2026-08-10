#!/usr/bin/env bash
# The DEPENDENCY-SCALE RUNTIME witness for the moving collector (#7717, the one
# unmet ask of #7280).
#
# Reproduce a CI failure with exactly this, from the repo root:
#
#   npm ci --ignore-scripts --no-audit --no-fund
#   cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
#   ./scripts/gc_dep_scale_witness.sh
#
# The env knobs live HERE and not in the YAML on purpose: a retyped invocation
# that drops one of them produces a run that exits 0 while witnessing nothing,
# which is the failure this file exists to prevent.
#
# WHY A RUNTIME WITNESS, WHEN TWO STATIC GATES ALREADY COMPILE THIS CORPUS
#
# `gc-root-dominance` compiles `test-files/gc-dep-corpus/` and checks the
# emitted LLVM IR. That cannot see a runtime-side cache holding a raw heap
# pointer — `scripts/gc_root_dominance_check.py` reads IR, and a `static` or
# `thread_local!` in perry-runtime is structurally invisible to it. The #7154
# class is also invisible to every runtime GC probe AT the collection, because
# there is nothing for the collector to find; it surfaces cycles later, in a
# different function, as `TypeError: value is not a function`.
#
# So the only instrument that can observe it is: run real dependency-shaped code
# under a collector that relocates constantly, with the from-space quarantine
# armed deep enough that a stale pointer faults at the faulting instruction
# instead of silently reading recycled bytes.
#
# WHY DEPENDENCY-SCALE RATHER THAN A FIXTURE. #7280 measured it: the curated
# 25-file corpus passed 25/25 while twenty lines of stock `zod` failed 5/40. It
# is a distribution problem, not a size one — the dependency-scale corpus is
# dominated by `js_object_assign_one` (object spread) and
# `js_new_function_construct`, populations the curated files barely produce.
#
# WHAT MAKES THIS ABLE TO FAIL (CLAUDE.md's four ways a gate cannot)
#
#   1. No `continue-on-error`, no `|| true`, no pipe between a producer and its
#      check — every command's status is the gate's status (`set -euo pipefail`).
#   2. It asserts its SUBJECT RAN, three independent ways, and any one of them
#      missing is a hard failure:
#        * the `[gc-zeal]` exit verdict, which since #7604 makes the binary
#          itself exit 70 when a zeal run forced no collection or relocated
#          nothing — so a vacuous run is red without this script's help;
#        * `copying_minors` and `loop_polls` are re-read here anyway, because
#          "the binary would have exited 70" is a claim about a version of the
#          binary, and this gate should not depend on that staying true;
#        * `[gc-fromspace-protect] retired_set=#N`, because
#          `PERRY_GC_PROTECT_FROMSPACE=1` on a run with no copying minor
#          protects NOTHING and still exits clean. #7717 records hitting exactly
#          that and nearly recording the wrong conclusion.
#   3. The answer is compared against the same binary run WITHOUT zeal. A stale
#      root corrupts values, so identical output across the two is the property
#      that matters. (It is not diffed against node: the corpus imports zod by
#      SOURCE PATH — `node_modules/zod/src/index.js` resolving to `.ts` — which
#      is what makes its modules native to Perry and what node cannot resolve.)
#   4. Promotion to a required context is deliberately NOT done here; a new gate
#      has never been green, so it is observed on `main` first.
#
# DEPTH 800 IS NOT A ROUND NUMBER. `PERRY_GC_PROTECT_FROMSPACE_DEPTH` bounds how
# many retired page-sets stay quarantined, and the default of 4 is far too small
# for real code: #7154's own reproducer needed 800 because the value crossed 600
# polls between its last valid observation and its stale use. Measured on this
# workload the quarantine runs saturated — `sets_held=800/800` — so the depth is
# load-bearing rather than decorative, and lowering it silently narrows the
# window in which a stale dereference can still be caught.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PERRY_BIN="${PERRY_BIN:-$ROOT/target/release/perry}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
ENTRY="${ENTRY:-test-files/gc-dep-corpus/main.ts}"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

fail() { echo "::error::$*" >&2; exit 1; }

[ -x "$PERRY_BIN" ] || fail "PERRY_BIN=$PERRY_BIN is not executable. Build: cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static"
for lib in libperry_runtime.a libperry_stdlib.a; do
  [ -s "$RUNTIME_DIR/$lib" ] || fail "$RUNTIME_DIR/$lib is missing. perry-runtime and perry-stdlib are rlib-only; the archives come from the -static wrapper crates."
done
# The dependency IS the test. Its absence must be an error rather than a
# silently smaller witness.
[ -f node_modules/zod/src/index.ts ] || fail "node_modules/zod/src/index.ts is missing — run: npm ci --ignore-scripts --no-audit --no-fund"
[ -f "$ENTRY" ] || fail "$ENTRY is missing"

export PERRY_RUNTIME_DIR="$RUNTIME_DIR"
# Same reason gc-moving-witnesses.yml sets it: PERRY_GC_MOVING_LOOP_POLLS is a
# compile-time gate that build_cache.rs does not key (#7183), so the build-level
# cache cannot tell a polled binary from an unpolled one. The object cache does
# key it; disabling the build cache costs one compile here.
export PERRY_DISABLE_BUILD_CACHE=1
# THIS ONE IS NOT OPTIONAL, and it is where this script deliberately departs
# from `scripts/gc_repsel_matrix.sh`, which omits it on purpose.
#
# Measured while writing this: without it the compile reaches the
# auto-optimizer, which RELINKS the runtime as
# `features=async-runtime,web-fetch` — note what is absent. `diagnostics` is
# what emits `[gc-fromspace-protect]`, i.e. the only evidence that the
# quarantine engaged at all, and it is one of the three things this gate
# asserts. The matrix can afford to let the auto-optimizer choose because it
# re-reads whichever trace format it gets; this witness cannot, because a
# stripped runtime removes its evidence rather than changing its shape.
#
# The assertions below are fail-closed, so a stripped runtime would go red
# rather than quietly pass — but red-for-the-wrong-reason is still a broken
# gate, and the relink also costs minutes of CI on every run.
export PERRY_NO_AUTO_OPTIMIZE=1

BIN="$OUT_DIR/gc-dep-witness"
echo "==> compiling $ENTRY"
"$PERRY_BIN" "$ENTRY" -o "$BIN" || fail "the witness workload did not compile"
[ -x "$BIN" ] || fail "$BIN was not produced"

echo "==> baseline run (no zeal), for the answer to be compared against"
"$BIN" > "$OUT_DIR/plain.out" 2> "$OUT_DIR/plain.err" \
  || fail "the workload failed WITHOUT zeal — that is a plain breakage, not a rooting witness. stderr: $(tail -5 "$OUT_DIR/plain.err")"
[ -s "$OUT_DIR/plain.out" ] || fail "the workload printed nothing; there is no answer to compare"

echo "==> witness run: zeal + from-space quarantine at depth 800"
# PERRY_GC_DIAG=1 is what emits `[gc-fromspace-protect]`, which is the only
# evidence that the quarantine engaged. Both are eprintln-only diagnostics.
set +e
PERRY_GC_ZEAL=1 \
PERRY_GC_PROTECT_FROMSPACE=1 \
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 \
PERRY_GC_DIAG=1 \
  "$BIN" > "$OUT_DIR/zeal.out" 2> "$OUT_DIR/zeal.err"
status=$?
set -e

if [ "$status" -ne 0 ]; then
  echo "--- last 40 lines of witness stderr ---" >&2
  tail -40 "$OUT_DIR/zeal.err" >&2 || true
  case "$status" in
    70) fail "the zeal run exercised nothing (exit 70). This is NOT a pass: see the [gc-zeal] lines above for which of forced_collections / copying_minors / loop_polls was zero." ;;
    139|134|11) fail "the witness FAULTED under the from-space quarantine (exit $status). This is the #7154 class doing what it does: a stale from-space pointer was dereferenced. The reporter above names the address, the retiring minor, and the last-known object's type." ;;
    *) fail "the witness exited $status under zeal" ;;
  esac
fi

if ! diff -u "$OUT_DIR/plain.out" "$OUT_DIR/zeal.out" > "$OUT_DIR/answer.diff"; then
  echo "--- answer differs between the plain and zeal runs ---" >&2
  cat "$OUT_DIR/answer.diff" >&2
  fail "the workload computed a DIFFERENT answer under a relocating collector. A stale root corrupts values; this is the symptom without the fault."
fi

verdict="$(grep -F '[gc-zeal] forced_collections=' "$OUT_DIR/zeal.err" | tail -1 || true)"
[ -n "$verdict" ] || fail "no [gc-zeal] verdict line was printed, so nothing proves a collection happened. Was PERRY_GC_ZEAL=1 actually set, and is this binary new enough to emit it (#7604)?"

read_field() { echo "$verdict" | grep -oE "$1=[0-9]+" | cut -d= -f2; }
cycles="$(read_field copying_minors)"
moved="$(read_field moved_objects)"
polls="$(read_field loop_polls)"
: "${cycles:=0}" "${moved:=0}" "${polls:=0}"

[ "$cycles" -gt 0 ] || fail "copying_minors=0 — nothing was relocated, so a stale-root witness could not have failed no matter how broken the rooting is."
[ "$moved" -gt 0 ] || fail "moved_objects=0 — the collector ran but relocated nothing, which is the same vacuous result."
# Back-edge polls are what put a collection INSIDE a loop body. Without them a
# zeal run only collects at event-loop boundaries and no loop is covered.
[ "$polls" -gt 0 ] || fail "loop_polls=0 — every collection came from an event-loop boundary, so no loop body was covered. Codegen emits no poll for a provably alloc-free body, nor for the specialized for/for-of/for-in lowerings."

retired="$(grep -cF '[gc-fromspace-protect]' "$OUT_DIR/zeal.err" || true)"
: "${retired:=0}"
[ "$retired" -gt 0 ] || fail "the from-space quarantine never retired a page-set, so PERRY_GC_PROTECT_FROMSPACE=1 protected NOTHING and this run's cleanliness means nothing (#7717)."

echo
echo "$verdict"
echo "[gc-dep-scale-witness] retired page-sets: $retired"
echo "[gc-dep-scale-witness] answer identical with and without a relocating collector:"
sed 's/^/    /' "$OUT_DIR/plain.out"
echo "[gc-dep-scale-witness] PASS"
