#!/usr/bin/env bash
# GC x representation-selection stress matrix.
#
# WHY THIS EXISTS
# ---------------
# The representation-selection campaign shipped several new value
# representations (canonical unboxed i32 locals #6903, tagged-at-rest `Str`
# #6909, `Ptr<Shape>` #6911, `Ptr<NumArray>` #6915/#6916, spec-ABI raw params
# #6905, native-i32 residency #6898). Each made its own GC-safety argument in
# its own PR, verified once by hand at merge time. Meanwhile the collector
# changed underneath all of them (#6910 mark/rewrite root-word parity, #6921
# typed-shape layout on the ctor exit, #6892 minor-sweep finalization, #6655
# operand rooting). Nobody verified the cross-product. This script is that
# cross-product, as a maintained gate.
#
# ***LIVENESS IS PART OF THE RESULT.***
# Setting a GC env var does not prove the GC did anything (#6942, #6946, #6950).
# Without the pressure knob a gap test allocates a few KB against a first-GC
# trigger that needs ~1M escaping allocations, so every GC arm is INERT against
# it and "passes under PERRY_GC_FORCE_EVACUATE=1" asserts nothing. That is what
# `--pressure` is for, and it is not sufficient on its own -- see the liveness
# table every run prints, and the known-inert registry it is checked against.
#
# Therefore every run is executed with `PERRY_GC_TRACE=1` (one `[gc] cycle`
# marker per completed collection -- a sound cycle counter) and `PERRY_GC_DIAG=1`
# (`moved_objects=` / `[gc-copy-minor] ran` -- evacuation evidence), and each
# cell is reported as one of:
#
#   PASS   output byte-exact vs the pinned Node oracle AND the arm's liveness
#          requirement was met on this test
#   UNVER  output byte-exact but the arm was INERT here (no collection, or
#          nothing moved for an evacuating arm) -- NOT green, by design
#   XFAIL  a triaged, justified expected-red (test-parity/gc_repsel_triage.txt)
#   FAIL   output mismatch, crash, non-zero exit, or compile failure
#
# A cell that matched the oracle under an inert arm is UNVER, never PASS. A
# matrix of green cells from inert arms would license exactly the false
# confidence this gate exists to remove.
#
# `test_gap_repsel_gc_stress` is the corpus member deliberately built to be
# LIVE: it collects even in the shipped configuration with no pressure knob at
# all, which almost nothing else in the corpus does. If a collector change stops
# it collecting, its cells go UNVER, the arm liveness summary drops, and the
# liveness gate fails -- that is the signal to re-tune its churn budget. (Its
# per-run cycle counts belong in the run's own output, not here.)
#
# ADDING A REPRESENTATION: register its gap file in
# test-parity/gc_repsel_corpus.txt. This script FAILS if a `test_gap_repsel_*`
# or `test_gap_specabi_*` file exists that is not registered (see
# docs/representation-selection-rfc.md 5.6).
#
# ***AND LIVENESS IS NOW GATED, NOT MERELY REPORTED (#7255).***
# UNVER was "not green" but it was never red either: the exit status counts only
# FAIL, so an arm that went inert across the WHOLE corpus produced a yellow table
# and exit 0. Every run therefore ends in
# `scripts/gc_matrix_liveness_check.py`, which fails when an arm satisfied its
# own `requires=` on zero cells — and equally when an arm listed in
# `test-parity/gc_matrix_inert_arms.txt` starts biting again, so the registry
# cannot rot in the other direction either.
#
# ***DO NOT PUT LIVENESS NUMBERS IN THIS HEADER.*** The last hand-maintained
# pair (`default: 0/22 -> 12/22` after #7024) read as settled fact for five weeks
# after #7161 took that same arm back to 0/49, and it is the reason nobody
# re-derived it (#7255). The per-arm table printed by every run is the only place
# those numbers are true, and it is derived from the collector's own output.
#
# Portable to bash 3.2 (macOS system bash): no associative arrays, no mapfile.
#
# Usage:
#   scripts/gc_repsel_matrix.sh [--arms pr|all|<csv>] [--filter <substr>]
#                               [--pressure <MB>] [--jobs N] [--no-build]
#                               [--profile <cargo profile>] [--json <path>]
#                               [--list-arms] [--liveness-report-only]
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

MANIFEST="test-parity/gc_repsel_corpus.txt"
TRIAGE="test-parity/gc_repsel_triage.txt"
ARMS_SEL="pr"
FILTER=""
PRESSURE_MB="8"
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"
DO_BUILD=1
JSON_OUT=""
PROFILE="release"
LIVENESS_REPORT_ONLY=0

while [ $# -gt 0 ]; do
    case "$1" in
        --arms) ARMS_SEL="$2"; shift 2 ;;
        --filter) FILTER="$2"; shift 2 ;;
        --pressure) PRESSURE_MB="$2"; shift 2 ;;
        --jobs) JOBS="$2"; shift 2 ;;
        --profile) PROFILE="$2"; shift 2 ;;
        --no-build) DO_BUILD=0; shift ;;
        --json) JSON_OUT="$2"; shift 2 ;;
        --list-arms) ARMS_SEL="__list__"; shift ;;
        # Local exploration only (e.g. a `--filter` narrow enough that an arm
        # legitimately has nothing to bite). CI never passes this: the whole
        # point of #7255 is that an inert arm must be able to turn a run red.
        --liveness-report-only) LIVENESS_REPORT_ONLY=1; shift ;;
        -h|--help) sed -n '1,70p' "$0"; exit 0 ;;
        *) echo "unknown flag: $1" >&2; exit 2 ;;
    esac
done

RED=$'\033[0;31m'; GREEN=$'\033[0;32m'; YELLOW=$'\033[0;33m'; NC=$'\033[0m'
[ -t 1 ] || { RED=""; GREEN=""; YELLOW=""; NC=""; }

# ---------------------------------------------------------------------------
# Arms.  Format:  id | compile-env | run-env | liveness-requirement | note
#
# liveness requirement:
#   scavenge the arm claims the COPYING MINOR runs -> require
#            `[gc-copy-minor] ran copied_objects=` > 0. Strictly stronger than
#            `move`, which the C4b mark-sweep evacuation satisfies on its own
#            (#7025) -- `default` reported `moved=7 610 512` while running zero
#            copying minors. Any arm whose subject is the relocating young-gen
#            minor #7019 shipped must use THIS, not `move`.
#   move     the arm claims to evacuate -> require moved/copied objects > 0
#   collect  the arm claims to collect  -> require at least one GC cycle
#   none     no GC claim of its own (an explicit control)
#
# %P% expands to the pressure env (PERRY_GC_HEAP_LIMIT=<MB>) unless
# --pressure 0. `-` means "no run env at all". Compile-time vars change emitted
# IR; all of them are keyed into the object cache
# (perry/src/commands/compile/object_cache.rs), so arms never silently share
# cached objects.
#
# %E% expands to THE EVACUATING BASE (#6950):
#
#     PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off
#
# Every arm whose requirement is `move` carries it, because without it those
# arms are INERT and were reported UNVER across the whole corpus. Two
# independent blockers, both measured:
#
#   1. `PERRY_GC_INCREMENTAL=0`. With incremental mode on (the default),
#      `registered_root_scanners_block_budgeted_gc()` reduces to "any copy-only
#      scanner", which a compiled program has none of. So `gc_check_trigger`
#      skips the direct-collection arm and hands the trigger to the budgeted
#      stepper, whose mutator assists never drive the cycle to completion.
#      Turning incremental off restores the direct synchronous minor.
#   2. `PERRY_CONSERVATIVE_STACK_SCAN=off`. The direct arm takes
#      `ManualGcScanGuard::force_full_scan()`, and a forced conservative scan
#      makes the copying minor ineligible (`fallback=conservative_stack`) --
#      the exact sentence #6950 quotes from `gc/policy.rs`. An explicit env
#      value BEATS that guard in `conservative_stack_scan_mode()`, so this is
#      what turns the automatic collection into a precise-rooted copying minor
#      that actually relocates survivors.
#
# Every %E% arm reports `[gc-copy-minor] eligible=true fallback=none`; the
# per-run liveness table at the bottom of the output says how many cells that
# was, on the tree you are actually running. (There used to be a hard-coded
# before/after table here. It was true when written and false a month later --
# see the DO NOT PUT LIVENESS NUMBERS IN THIS HEADER note above, and #7255.)
#
# NOTE this is a MEASUREMENT configuration, not the shipped one. It says the
# collector's evacuating path is exercised; it does not say the shipped default
# reaches that path.
#
# THE SHIPPED DEFAULT DOES NOT REACH IT TODAY. #7019/#7024 made it reach it by
# the sound route -- defer the alloc-point trigger to a precise-root safepoint
# and run the copying minor there -- and #7161 then turned that route off by
# default, pending #7154. So the WHERE distinction still stands and still
# matters (a safepoint has an unwound JS stack and roots precise by
# construction; %E% forces relocation at the register-imprecise allocation
# point, which is the only place an unrooted runtime-side local is exposed),
# but the arm that carries the safepoint route is `safepoint_minor`, which opts
# the polls back in at compile AND run time. `default` is registered known-inert
# in test-parity/gc_matrix_inert_arms.txt until the stopgap lifts.
#
# ***AND WHEN THESE ARMS FIRST MOVED, THEY WERE RED.*** The first `--arms all`
# run in which anything actually moved failed 14 of the 20 corpus files then in
# the corpus: 5 crashes and 9 output mismatches (#6981), plus one intermittent
# SIGSEGV that does not even need precise roots (#6982). The discriminator is
# NOT relocation -- with the conservative stack scan still on, the same
# evacuating cycles passed 19/20 while copying thousands of objects. It is
# precise roots: the values only the conservative scan was keeping alive. That
# is the finding this gate was built to produce, and the arms stay configured to
# keep producing it. Do not quiet them down -- and note that "quiet" now has a
# second, cheaper failure mode than lowering a requires=: letting an arm go
# inert. The liveness gate exists because that one is invisible on screen.
# ---------------------------------------------------------------------------
ARMS=(
"default||%P%|scavenge|as-shipped GC configuration under allocation pressure. ***INERT AT THE MOMENT, AND REGISTERED AS SUCH*** in test-parity/gc_matrix_inert_arms.txt. #7024 made this a relocating arm (the alloc-point trigger defers to js_gc_loop_safepoint -> gc_safepoint_moving_minor, which runs the copying minor on precise rewritable roots); #7161 then flipped PERRY_GC_MOVING_LOOP_POLLS default-OFF pending #7154, and that one env gates BOTH halves of the route -- perry-codegen's moving_safepoint_polls_enabled decides whether the back-edge polls are emitted at all, and perry-runtime's gc_moving_loop_polls_enabled decides whether the trigger defers to them. A default binary has neither, so the minor runs behind ManualGcScanGuard::force_full_scan and the copying minor is ineligible by construction. requires=scavenge STAYS: it is what the shipped default is FOR, the registry entry names what blocks it, and the liveness gate fails the day it scavenges again so the entry cannot outlive its cause. safepoint_minor carries the relocating claim meanwhile."
"safepoint_minor|PERRY_GC_MOVING_LOOP_POLLS=1|%P% PERRY_GC_MOVING_LOOP_POLLS=1|scavenge|THE SOUND RELOCATING ARM, and what keeps the #6993 defect class reachable per-PR while #7161's stopgap holds. Sets the poll flag at BOTH compile and run time (same env on both sides -- keyed into the object cache as env_gc_moving_loop_polls, so a warm cache cannot serve poll-free objects). The copying minor then runs at js_gc_loop_safepoint -> gc_safepoint_moving_minor, where the loop body has completed and every live heap value is a named local on the shadow stack: precise, rewritable roots. No %E%, no force -- this is exactly what default was between #7024 and #7161, and what default becomes again when the stopgap lifts. NOT a replacement for the %E% arms: a back-edge poll only fires while user JS runs, so it cannot expose an unrooted local inside runtime code that never re-enters user JS (#7249). evac_minor and force_verify remain in the PR subset for that."
"evac_minor||%P% %E%|move|THE evacuating arm, and the STRONGER acceptance route (#7249): the automatic alloc-point collection as a COPYING minor that relocates survivors at a register-imprecise point, which is where an unrooted runtime-side local is exposed. No stress knob -- this is the collector's own moving path."
"force_evac||%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|stress-copy every marked non-pinned nursery object"
"verify_evac||%P% PERRY_GC_VERIFY_EVACUATION=1|scavenge|panic if a live slot still points at a forwarded object. requires=scavenge: a verifier that runs over zero relocations verifies nothing. REGISTERED KNOWN-INERT (#7161) -- same route as default, same blocker, and the same reason the declaration is not being weakened to hide it."
"force_verify||%P% %E% PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1|move|force + verify"
"gen_gc_off||%P% PERRY_GEN_GC=0|collect|full mark-sweep only; no nursery => no evacuation by construction"
"wb_off|PERRY_WRITE_BARRIERS=0|%P% PERRY_WRITE_BARRIERS=0|collect|no codegen write barriers => copying nursery ineligible by construction"
"gen_off_verify||%P% PERRY_GEN_GC=0 PERRY_GC_VERIFY_EVACUATION=1|collect|full mark-sweep + evacuation verifier"
"wb_off_force|PERRY_WRITE_BARRIERS=0|%P% PERRY_WRITE_BARRIERS=0 PERRY_GC_FORCE_EVACUATE=1|collect|force-evacuate is a documented no-op without barriers (barriers_inactive)"
"all_four|PERRY_WRITE_BARRIERS=0|%P% PERRY_GEN_GC=0 PERRY_WRITE_BARRIERS=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1|collect|every escape hatch at once"
"cons_scan_off||%P% PERRY_CONSERVATIVE_STACK_SCAN=off|scavenge|PRECISE ROOTS ONLY -- removes the conservative-stack pinning that the alloc-point fallback otherwise forces (ManualGcScanGuard::force_full_scan). An arm that can observe a missing shadow-slot binding. REGISTERED KNOWN-INERT (#7161): precise roots beat that guard, but with the incremental stepper at its default the nursery trigger never reaches the direct arm in the first place -- registered_root_scanners_block_budgeted_gc() reduces to 'any copy-only scanner' under gc_incremental_enabled(), a compiled program has none, so the trigger goes to the budgeted stepper, which is non-moving by construction. Adding PERRY_GC_INCREMENTAL=0 is what turns it live, and that arm is evac_minor."
"cons_scan_off_force||%P% PERRY_CONSERVATIVE_STACK_SCAN=off PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1|scavenge|precise roots + force/verify evacuation. REGISTERED KNOWN-INERT (#7161), same reason as cons_scan_off: PERRY_GC_FORCE_EVACUATE is read on a minor path this arm never reaches, which is the #6942/#6946 shape exactly."
"loop_polls|PERRY_GC_MOVING_LOOP_POLLS=1|%P% %E% PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_FORCE_EVACUATE=1|move|defer the alloc-point collection to a loop back-edge precise-root safepoint, where the copying minor may MOVE survivors"
"rep_i32_off|PERRY_CANONICAL_I32_LOCALS=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|repsel Phase 1 OFF x evacuation"
"rep_str_off|PERRY_CANONICAL_STR_LOCALS=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|repsel Phase 3a OFF x evacuation"
"rep_str_static_off|PERRY_STATIC_STRING_LOWERING=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|#7128 static-string lowerings OFF x evacuation -- the inline StringRef retag, the proven-heap operand handle and the tag-dispatched .length. Split off PERRY_CANONICAL_STR_LOCALS because they key on a value's static string type, not on a selected Str local; this arm is what keeps the off-state exercised."
"rep_ptr_shape_off|PERRY_PTR_SHAPE_LOCALS=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|repsel Phase 3b OFF x evacuation"
"rep_ptr_numarray_off|PERRY_PTR_NUMARRAY_LOCALS=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|repsel Phase 4a.3 OFF x evacuation"
"rep_spec_abi_off|PERRY_SPECIALIZED_ABI=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|repsel Phase 2 OFF x evacuation"
"rep_int_valued_off|PERRY_INT_VALUED_LOCALS=0|%P% %E% PERRY_GC_FORCE_EVACUATE=1|move|native-i32 residency (#6898) OFF x evacuation"
"shipped_default||-|none|control: exactly the as-shipped configuration -- no pressure knob, no GC env at all"
)

# PR-gating subset: the arms with the most detection power per second -- the
# shipped configuration under pressure, the two routes that actually relocate,
# the evacuation verifier, precise-roots-only, and the untouched shipped
# configuration as a control.
#
# WHAT THIS SUBSET MUST BE ABLE TO DO, AND HOW THAT IS ENFORCED
# ------------------------------------------------------------
# It must be able to reproduce the relocating-minor defect class (#6993: #6951,
# #6972, #6982, #6991, #6992 -- "a raw reference held across a relocating
# collection"). Before #7024 it could not, and the previous revision of this
# comment recorded the fix in bold, with the measurement that justified it:
# `default` had gone from copy-minor 0/22 to 12/22.
#
# ***THAT SENTENCE OUTLIVED ITS MEASUREMENT BY FIVE WEEKS (#7255).*** #7161
# flipped PERRY_GC_MOVING_LOOP_POLLS default-OFF pending #7154, which took
# `default` -- and `verify_evac`, `cons_scan_off`, `cons_scan_off_force` -- back
# to copy-minor 0 across the whole corpus. The comment still said 12/22, and
# because an all-UNVER table exits 0, nothing else said anything. Two open crash
# reports (#6982, #7018) sat un-judgeable for the duration because both fail
# INSIDE a copying minor and the arms meant to run one ran none.
#
# So the property is no longer asserted in prose here. It is enforced, twice:
#
#   * scripts/gc_matrix_liveness_check.py --check-registry (from `lint`, no
#     build) fails unless PR_ARMS contains at least one arm that claims to
#     relocate AND is not on the known-inert list. Registering every inert arm
#     is therefore not a way to buy a green subset.
#   * the same checker, run against every matrix invocation, fails when any arm
#     satisfied its own requires= on zero cells.
#
# WHY THE RELOCATING ARMS ARE THE ONES THEY ARE
# ---------------------------------------------
# `safepoint_minor` is the SOUND route (precise roots at a loop back-edge) and
# is what `default` becomes again when #7161's stopgap lifts. `evac_minor` and
# `force_verify` are the STRONGER acceptance route (#7249): a back-edge poll
# only fires while user JS runs, so it cannot expose an unrooted local inside
# runtime code that never re-enters user JS -- the register-imprecise
# allocation point can. Both kinds are in, deliberately; neither substitutes for
# the other.
#
# `default`, `verify_evac` and `cons_scan_off` STAY in the subset while they are
# registered known-inert. They still verify byte-exactness against the oracle
# under a collecting GC, they cost one run each, and leaving them in is what
# makes the registry entry visible on every PR instead of quietly true.
PR_ARMS="default,safepoint_minor,evac_minor,verify_evac,force_verify,cons_scan_off,shipped_default"

arm_field() { # $1 = arm record, $2 = 1..5
    printf '%s' "$1" | cut -d'|' -f"$2"
}

if [ "$ARMS_SEL" = "__list__" ]; then
    printf '%-24s %-9s %s\n' "ARM" "REQUIRES" "NOTE"
    for rec in "${ARMS[@]}"; do
        printf '%-24s %-9s %s\n' "$(arm_field "$rec" 1)" "$(arm_field "$rec" 4)" "$(arm_field "$rec" 5)"
    done
    exit 0
fi

case "$ARMS_SEL" in
    pr)  SELECTED="$PR_ARMS" ;;
    all) SELECTED="$(for rec in "${ARMS[@]}"; do printf '%s,' "$(arm_field "$rec" 1)"; done)" ;;
    *)   SELECTED="$ARMS_SEL" ;;
esac
SELECTED="${SELECTED%,}"

# ---------------------------------------------------------------------------
# Corpus + registration enforcement.
# ---------------------------------------------------------------------------
[ -f "$MANIFEST" ] || { echo "missing corpus manifest $MANIFEST" >&2; exit 2; }
CORPUS=()
while IFS= read -r line; do
    line="${line%%#*}"
    line="$(printf '%s' "$line" | tr -d '[:space:]')"
    [ -n "$line" ] && CORPUS+=("$line")
done < "$MANIFEST"

missing_reg=0
for f in test-files/test_gap_repsel_*.ts test-files/test_gap_specabi_*.ts; do
    [ -f "$f" ] || continue
    b="$(basename "$f" .ts)"
    found=0
    for c in "${CORPUS[@]}"; do [ "$c" = "$b" ] && found=1 && break; done
    if [ "$found" = 0 ]; then
        echo "${RED}UNREGISTERED${NC} $f is a representation-selection gap file but is not in $MANIFEST" >&2
        missing_reg=1
    fi
done
if [ "$missing_reg" = 1 ]; then
    echo "A NEW REPRESENTATION MUST REGISTER ITS GAP FILE in $MANIFEST." >&2
    echo "See docs/representation-selection-rfc.md 5.6 (GC under unboxed representations)." >&2
    exit 3
fi

for b in "${CORPUS[@]}"; do
    [ -f "test-files/$b.ts" ] || { echo "manifest lists $b but test-files/$b.ts does not exist" >&2; exit 3; }
done

if [ -n "$FILTER" ]; then
    FILTERED=()
    for b in "${CORPUS[@]}"; do
        case "$b" in *"$FILTER"*) FILTERED+=("$b") ;; esac
    done
    CORPUS=(${FILTERED[@]+"${FILTERED[@]}"})
fi
[ "${#CORPUS[@]}" -gt 0 ] || { echo "empty corpus after filter" >&2; exit 2; }

# ---------------------------------------------------------------------------
# Oracle + compiler.  THE ORACLE VERSION IS LOAD-BEARING: a test the oracle
# cannot run would drop out of the gate silently, so refuse to run at all.
# ---------------------------------------------------------------------------
PINNED_NODE="$(tr -d 'v \n' < .node-version 2>/dev/null || true)"
NODE_V="$(node --version 2>/dev/null | tr -d 'v \n')"
[ -n "$NODE_V" ] || { echo "node not on PATH" >&2; exit 2; }
if [ -n "$PINNED_NODE" ] && [ "$NODE_V" != "$PINNED_NODE" ]; then
    echo "${RED}ORACLE MISMATCH${NC}: node $NODE_V but .node-version pins $PINNED_NODE" >&2
    exit 2
fi

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
PERRY_BIN="$TARGET_DIR/$PROFILE/perry"
if [ "$DO_BUILD" = 1 ]; then
    echo "==> cargo build --profile $PROFILE (perry + runtime/stdlib staticlibs)"
    cargo build --profile "$PROFILE" --quiet \
        -p perry -p perry-runtime -p perry-stdlib \
        -p perry-runtime-static -p perry-stdlib-static \
        || { echo "${RED}build failed${NC}" >&2; exit 2; }
fi
[ -x "$PERRY_BIN" ] || { echo "${RED}missing $PERRY_BIN${NC}" >&2; exit 2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/perry-gcmatrix.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/oracle" "$WORK/bin" "$WORK/out"

echo "==> oracle: node $NODE_V x ${#CORPUS[@]} corpus files"
oracle_fail=0
for b in "${CORPUS[@]}"; do
    if ! node --experimental-strip-types "test-files/$b.ts" > "$WORK/oracle/$b.out" 2>/dev/null; then
        echo "${RED}ORACLE FAIL${NC} node cannot run test-files/$b.ts -- it would drop out of the gate" >&2
        oracle_fail=1
    fi
done
[ "$oracle_fail" = 0 ] || exit 2

# ---------------------------------------------------------------------------
# Arm selection + compile groups (one compile pass per distinct compile env).
# ---------------------------------------------------------------------------
ARM_IDS=(); ARM_CENVS=(); ARM_RENVS=(); ARM_LIVES=(); ARM_NOTES=(); ARM_SLUGS=()
GROUP_SLUGS=(); GROUP_ENVS=()
for rec in "${ARMS[@]}"; do
    id="$(arm_field "$rec" 1)"
    case ",$SELECTED," in *",$id,"*) ;; *) continue ;; esac
    cenv="$(arm_field "$rec" 2)"
    slug="$(printf '%s' "${cenv:-_base}" | tr -c 'A-Za-z0-9' '_')"
    ARM_IDS+=("$id"); ARM_CENVS+=("$cenv"); ARM_RENVS+=("$(arm_field "$rec" 3)")
    ARM_LIVES+=("$(arm_field "$rec" 4)"); ARM_NOTES+=("$(arm_field "$rec" 5)")
    ARM_SLUGS+=("$slug")
    known=0
    for g in ${GROUP_SLUGS[@]+"${GROUP_SLUGS[@]}"}; do [ "$g" = "$slug" ] && known=1 && break; done
    if [ "$known" = 0 ]; then GROUP_SLUGS+=("$slug"); GROUP_ENVS+=("$cenv"); fi
done
NARMS="${#ARM_IDS[@]}"
[ "$NARMS" -gt 0 ] || { echo "no arms selected ($ARMS_SEL)" >&2; exit 2; }

# Warm the auto-optimize archive serially first: the parallel fan-out below
# would otherwise have N processes racing to build the same target/perry-auto-*
# archive on a cold tree. The harness deliberately does NOT set
# PERRY_NO_AUTO_OPTIMIZE, so the binaries under test are linked exactly the way
# a shipped `perry file.ts` links them (that also decides whether the runtime
# carries the `diagnostics` feature, which changes the GC trace format -- both
# are handled below).
echo "==> warming the auto-optimize archive"
mkdir -p "$WORK/bin/_warm"
"$PERRY_BIN" "test-files/${CORPUS[0]}.ts" -o "$WORK/bin/_warm/warm" > "$WORK/bin/_warm/warm.log" 2>&1 \
    || { echo "${RED}warm-up compile failed${NC} (see $WORK/bin/_warm/warm.log)" >&2; }

echo "==> compiling ${#CORPUS[@]} files x ${#GROUP_SLUGS[@]} compile-env groups (jobs=$JOBS)"
gi=0
while [ "$gi" -lt "${#GROUP_SLUGS[@]}" ]; do
    slug="${GROUP_SLUGS[$gi]}"; cenv="${GROUP_ENVS[$gi]}"
    mkdir -p "$WORK/bin/$slug"
    printf '%s\n' "${CORPUS[@]}" | WORK="$WORK" PERRY_BIN="$PERRY_BIN" CENV="$cenv" SLUG="$slug" \
        xargs -P "$JOBS" -I{} sh -c \
        'env $CENV "$PERRY_BIN" "test-files/$1.ts" -o "$WORK/bin/$SLUG/$1" > "$WORK/bin/$SLUG/$1.log" 2>&1 || echo "COMPILEFAIL $SLUG $1"' _ {}
    gi=$((gi+1))
done

# ---------------------------------------------------------------------------
# Run + classify.  CELLS / EVID are flat arrays indexed test*NARMS + arm.
# ---------------------------------------------------------------------------
PRESSURE_ENV=""
[ "$PRESSURE_MB" != "0" ] && PRESSURE_ENV="PERRY_GC_HEAP_LIMIT=$PRESSURE_MB"
# The evacuating base -- see the %E% note above the arm table. Both halves are
# required and neither is sufficient alone.
EVAC_ENV="PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off"

triage_reason() { # $1 test, $2 arm
    [ -f "$TRIAGE" ] || return 1
    grep -v '^[[:space:]]*#' "$TRIAGE" 2>/dev/null \
        | awk -F'|' -v t="$1" -v a="$2" '
            { gsub(/^[ \t]+|[ \t]+$/, "", $1); gsub(/^[ \t]+|[ \t]+$/, "", $2);
              if ($1 == t && $2 == a) { sub(/^[ \t]+/, "", $3); print $3; found=1 } }
            END { exit(found ? 0 : 1) }'
}

CELLS=(); EVID=(); CYC=(); EVA=(); SCA=()
n_pass=0; n_unver=0; n_fail=0; n_xfail=0
ai=0
while [ "$ai" -lt "$NARMS" ]; do
    id="${ARM_IDS[$ai]}"; slug="${ARM_SLUGS[$ai]}"; live="${ARM_LIVES[$ai]}"
    renv="$(printf '%s' "${ARM_RENVS[$ai]}" | sed -e "s/%P%/$PRESSURE_ENV/" -e "s/%E%/$EVAC_ENV/")"
    [ "$renv" = "-" ] && renv=""
    echo "==> arm $id"
    ti=0
    while [ "$ti" -lt "${#CORPUS[@]}" ]; do
        b="${CORPUS[$ti]}"; bin="$WORK/bin/$slug/$b"; idx=$((ti*NARMS+ai))
        cycles=0; evacuated=0; scavenged=0
        if [ ! -x "$bin" ]; then
            result="FAIL"; ev="compile-failed"
        else
            # One run per cell: stdout is the parity artifact, stderr carries
            # both liveness signals (PERRY_GC_TRACE=1 -> one `[gc] cycle`
            # marker per collection; PERRY_GC_DIAG=1 -> evacuation counters).
            # shellcheck disable=SC2086
            env $renv PERRY_GC_TRACE=1 PERRY_GC_DIAG=1 "$bin" \
                > "$WORK/out/$b.$id.out" 2> "$WORK/out/$b.$id.err"
            rc=$?
            # Two trace formats exist: a runtime staticlib built WITHOUT the
            # `diagnostics` feature prints one `[gc] cycle (…disabled…)` marker
            # per collection; one built WITH it prints the full JSON trace
            # object. Count either -- both are exactly one line per cycle.
            cycles=$(grep -cE '^\[gc\] cycle|^\{.*"phase_progression"' "$WORK/out/$b.$id.err" 2>/dev/null | tr -d ' ')
            # #7025: these are TWO different collectors and must be reported
            # separately. Summing them lets a `requires=move` cell go green on
            # relocation the arm was not testing:
            #   evacuated= : `moved_objects=` from the C4b evacuation policy
            #                inside the mark-sweep collector -- the pre-existing
            #                non-moving-minor path that relocates tenured objects
            #                during a full cycle.
            #   scavenged= : `[gc-copy-minor] ran copied_objects=` from the
            #                copying young-gen minor -- the path #7019 made
            #                default-on, and the one the evacuating arms exist
            #                to exercise.
            # A cell showing `evacuated=N scavenged=0` did relocate something,
            # but it did NOT run a copying minor, and the distinction is exactly
            # what tells you whether the arm bit.
            evacuated=$(grep -oE 'moved_objects=[0-9]+' "$WORK/out/$b.$id.err" 2>/dev/null \
                        | grep -oE '[0-9]+$' | awk '{s+=$1} END {print s+0}')
            scavenged=$(grep -oE '\[gc-copy-minor\] ran copied_objects=[0-9]+' "$WORK/out/$b.$id.err" 2>/dev/null \
                        | grep -oE '[0-9]+$' | awk '{s+=$1} END {print s+0}')
            : "${cycles:=0}"; : "${evacuated:=0}"; : "${scavenged:=0}"
            moved=$((evacuated + scavenged))
            ev="cycles=$cycles evacuated=$evacuated scavenged=$scavenged"
            if [ "$rc" -ne 0 ]; then
                result="FAIL"; ev="exit=$rc $ev"
            elif ! cmp -s "$WORK/out/$b.$id.out" "$WORK/oracle/$b.out"; then
                result="FAIL"; ev="output-mismatch $ev"
            else
                case "$live" in
                    # #7024/#7025: the copying minor's OWN counter, never the
                    # sum. An arm that certifies the relocating young-gen minor
                    # must not go green on a C4b mark-sweep evacuation.
                    scavenge) [ "$scavenged" -gt 0 ] && result="PASS" || result="UNVER" ;;
                    move)    [ "$moved" -gt 0 ] && result="PASS" || result="UNVER" ;;
                    collect) [ "$cycles" -gt 0 ] && result="PASS" || result="UNVER" ;;
                    *)       result="PASS" ;;
                esac
            fi
        fi
        if [ "$result" = "FAIL" ]; then
            if reason="$(triage_reason "$b" "$id")"; then
                result="XFAIL"; ev="$reason | $ev"
            fi
        fi
        CELLS[$idx]="$result"; EVID[$idx]="$ev"
        # The liveness gate reads these as NUMBERS, not by re-parsing `$ev`:
        # a triage reason is free text and has already contained `=`.
        CYC[$idx]="$cycles"; EVA[$idx]="$evacuated"; SCA[$idx]="$scavenged"
        case "$result" in
            PASS)  n_pass=$((n_pass+1)) ;;
            UNVER) n_unver=$((n_unver+1)) ;;
            XFAIL) n_xfail=$((n_xfail+1)); echo "  ${YELLOW}XFAIL${NC} $b" ;;
            FAIL)  n_fail=$((n_fail+1)); echo "  ${RED}FAIL${NC} $b ($ev)" ;;
        esac
        ti=$((ti+1))
    done
    ai=$((ai+1))
done

# ---------------------------------------------------------------------------
# Table + arm liveness summary.
# ---------------------------------------------------------------------------
echo
printf '%-40s' "test \\ arm"
for id in "${ARM_IDS[@]}"; do printf '%-8s' "$(printf '%s' "$id" | cut -c1-7)"; done
echo
ti=0
while [ "$ti" -lt "${#CORPUS[@]}" ]; do
    b="${CORPUS[$ti]}"
    printf '%-40s' "${b#test_gap_}"
    ai=0
    while [ "$ai" -lt "$NARMS" ]; do
        c="${CELLS[$((ti*NARMS+ai))]:-?}"
        case "$c" in
            PASS)  printf '%s%-8s%s' "$GREEN" "PASS" "$NC" ;;
            UNVER) printf '%s%-8s%s' "$YELLOW" "UNVER" "$NC" ;;
            XFAIL) printf '%s%-8s%s' "$YELLOW" "XFAIL" "$NC" ;;
            *)     printf '%s%-8s%s' "$RED" "$c" "$NC" ;;
        esac
        ai=$((ai+1))
    done
    echo
    ti=$((ti+1))
done

echo
echo "arm liveness across the corpus (cells where the arm actually bit):"
ai=0
while [ "$ai" -lt "$NARMS" ]; do
    tot=0; livec=0; livem=0; lives=0; ti=0
    while [ "$ti" -lt "${#CORPUS[@]}" ]; do
        ev="${EVID[$((ti*NARMS+ai))]:-}"
        cy="$(printf '%s' "$ev" | sed -nE 's/.*cycles=([0-9]+).*/\1/p')"
        evac="$(printf '%s' "$ev" | sed -nE 's/.*evacuated=([0-9]+).*/\1/p')"
        scav="$(printf '%s' "$ev" | sed -nE 's/.*scavenged=([0-9]+).*/\1/p')"
        tot=$((tot+1))
        [ "${cy:-0}" -gt 0 ] 2>/dev/null && livec=$((livec+1))
        [ $(( ${evac:-0} + ${scav:-0} )) -gt 0 ] 2>/dev/null && livem=$((livem+1))
        [ "${scav:-0}" -gt 0 ] 2>/dev/null && lives=$((lives+1))
        ti=$((ti+1))
    done
    # #7025: `copy-minor` is reported separately from `moved-objects` because the
    # latter counts BOTH collectors. An evacuating arm showing a healthy
    # moved-objects count but `copy-minor 0/N` did not run the path it exists to
    # test -- that is the shape #7024 describes, and summing the two hid it.
    printf '  %-24s requires=%-8s collected %2d/%2d   moved-objects %2d/%2d   copy-minor %2d/%2d\n' \
        "${ARM_IDS[$ai]}" "${ARM_LIVES[$ai]}" "$livec" "$tot" "$livem" "$tot" "$lives" "$tot"
    ai=$((ai+1))
done

echo
# Two DIFFERENT properties are being reported and they must not be conflated:
#   * byte-exactness vs the pinned Node oracle -- verified in every cell that
#     is not FAIL, including cells whose GC arm was inert. For the
#     representation-flag arms this IS the meaningful result: the rep's ON and
#     OFF lowerings agree byte-for-byte.
#   * the GC-stress property -- only verified where the arm was measurably
#     live. That is what PASS vs UNVER distinguishes.
n_byte_exact=$((n_pass + n_unver))
n_cells=$((n_pass + n_unver + n_xfail + n_fail))
echo "byte-exact vs node $NODE_V: $n_byte_exact/$n_cells cells (the parity property)"
echo "summary: PASS=$n_pass UNVER=$n_unver XFAIL=$n_xfail FAIL=$n_fail"
echo "  (pressure=${PRESSURE_MB}MB, node $NODE_V, $PERRY_BIN)"
echo "  UNVER = output matched but the arm was inert here; see #6942 / #6946 / #6950."

# The report is written UNCONDITIONALLY -- the liveness gate below consumes it,
# so `--json` only decides whether a copy is kept where the caller asked for it.
# A gate that runs only when someone remembered a flag is not a gate.
JSON_REPORT="${JSON_OUT:-$WORK/matrix.json}"
{
    printf '{"node":"%s","pressure_mb":"%s","arms":[' "$NODE_V" "$PRESSURE_MB"
    ai=0
    while [ "$ai" -lt "$NARMS" ]; do
        [ "$ai" = 0 ] || printf ','
        printf '{"id":"%s","requires":"%s"}' "${ARM_IDS[$ai]}" "${ARM_LIVES[$ai]}"
        ai=$((ai+1))
    done
    printf '],"cells":['
    first=1; ti=0
    while [ "$ti" -lt "${#CORPUS[@]}" ]; do
        ai=0
        while [ "$ai" -lt "$NARMS" ]; do
            [ "$first" = 1 ] || printf ','; first=0
            idx=$((ti*NARMS+ai))
            # Evidence is free text (triage reasons quote code); strip the two
            # characters that would make this invalid JSON, so a malformed
            # report can never be the reason the gate fails.
            ev_json="$(printf '%s' "${EVID[$idx]:-}" | tr '"\\' "''")"
            printf '{"test":"%s","arm":"%s","result":"%s","cycles":%d,"evacuated":%d,"scavenged":%d,"evidence":"%s"}' \
                "${CORPUS[$ti]}" "${ARM_IDS[$ai]}" "${CELLS[$idx]:-?}" \
                "${CYC[$idx]:-0}" "${EVA[$idx]:-0}" "${SCA[$idx]:-0}" "$ev_json"
            ai=$((ai+1))
        done
        ti=$((ti+1))
    done
    printf '],"summary":{"pass":%d,"unverified":%d,"xfail":%d,"fail":%d}}\n' \
        "$n_pass" "$n_unver" "$n_xfail" "$n_fail"
} > "$JSON_REPORT"
[ -n "$JSON_OUT" ] && echo "json: $JSON_OUT"

# ---------------------------------------------------------------------------
# LIVENESS GATE (#7255). Half of this script's exit status.
#
# `n_fail` alone cannot express "the arm never bit": an all-UNVER table is
# yellow on screen and exit 0 in CI, which is how four of the six PR-gating arms
# sat at copy-minor 0/49 for five weeks while this file's header advertised
# 12/22. The checker is a separate, self-testable program (it runs `--self-test`
# and `--check-registry` from `lint`, where no build is needed) so the rule that
# decides red-vs-green is not 30 lines of untested bash.
# ---------------------------------------------------------------------------
echo
liveness_rc=0
if command -v python3 > /dev/null 2>&1; then
    liveness_args=""
    [ "$LIVENESS_REPORT_ONLY" = 1 ] && liveness_args="--report-only"
    # shellcheck disable=SC2086
    python3 "$SCRIPT_DIR/gc_matrix_liveness_check.py" $liveness_args "$JSON_REPORT" || liveness_rc=1
else
    echo "${RED}LIVENESS GATE SKIPPED${NC}: python3 not on PATH." >&2
    echo "  This is a gate, not a report. Refusing to claim a green run without it." >&2
    liveness_rc=1
fi

[ "$n_fail" = 0 ] || exit 1
[ "$liveness_rc" = 0 ] || exit 1
exit 0
