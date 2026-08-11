#!/usr/bin/env bash
# The #7647 CI gate: promotes the "tape=0 + from-space-scan parse-then-churn"
# check (#7643's own follow-up note, descending from #7635) from a hand-run
# investigation into something CI runs on every relevant change.
#
# WHY THIS EXISTS
#
# `PERRY_JSON_TAPE=0` + `PERRY_GC_FROMSPACE_SCAN=1` over a parse-then-churn
# workload is a known-good end-to-end detector for the whole layout-state
# family: with the JSON materialiser's finalize sabotaged to always claim
# `POINTER_FREE` (#7635's exact mutation), it reports `dangling=8000
# owners=4000` and the binary SIGBUSes; clean, `dangling=0` and exit 0.
# #7643/#7644 shipped unit tests for two invariants that need no workload at
# all (the child-slot enumerator, and relocation of a hand-built sabotaged
# record) -- the right primary guard, because they cannot be defeated by a
# lazy path or a GC that did not happen to run. But neither can catch a NEW
# materialiser path that forgets to finalize at all, since such a path would
# simply not be exercised by a hand-built object. That is what this
# end-to-end gate is for, and until now nothing ran it in CI.
#
# THIS SCRIPT IS DESIGNED TO BE ABLE TO FAIL, checked against CLAUDE.md's
# "four ways a gate can be unable to fail":
#
#   1. no `continue-on-error`, no `|| true`, no pipe between a checker and
#      the shell's exit status -- see the final `exit` below.
#   2. NOT wired into branch protection's required contexts by this change.
#      A new gate has never been green, so promoting it immediately would
#      block every open PR -- that is a maintainer action for after one
#      observed green run on `main` (CLAUDE.md's corollary: the promotion
#      step must actually be taken, not left undone).
#   3. the workflow's `concurrency` block cancels pull_request runs only;
#      push (main) runs are keyed by commit SHA so they queue instead of
#      cancelling each other (see .github/workflows/gc-parse-churn-gate.yml).
#   4. the subject must be LIVE, not merely quiet. `PERRY_GC_FROMSPACE_SCAN`
#      only ever runs during a COPYING minor, and the moving collector is
#      opt-in (`PERRY_GC_MOVING_LOOP_POLLS=1`, both at compile time and run
#      time -- #7161's stopgap made this the ONLY configuration that
#      exercises it end to end). And `PERRY_JSON_TAPE=0` alone is a knob
#      setting, not proof it was honoured. So `scripts/gc_parse_churn_layout_check.py`
#      -- the actual pass/fail logic, invoked below -- rejects a run with
#      zero copying minors AND a run whose from-space census stayed small
#      enough that the record cohort was plainly still on the lazy tape.
#      Both are read from the SAME captured output the correctness check
#      uses, so there is no separate "did it run" side-channel to drift out
#      of sync.
#
# Usage: scripts/gc_parse_churn_layout_gate.sh [path-to-perry]
#   Expects a `perry` binary whose PERRY_RUNTIME_DIR-resolvable staticlibs
#   are current (see CLAUDE.md's "Verifying a runtime change" pitfall --
#   perry-runtime/perry-stdlib are rlib-only, the .a comes from the
#   *-static wrapper crates, and a stale archive makes this whole gate
#   vacuous in a way nothing here can detect).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PERRY_BIN="${1:-$REPO_ROOT/target/release/perry}"
if [[ ! -x "$PERRY_BIN" ]]; then
  echo "FAIL: no perry binary at $PERRY_BIN" >&2
  exit 1
fi
PERRY_BIN="$(cd "$(dirname "$PERRY_BIN")" && pwd)/$(basename "$PERRY_BIN")"
export PERRY_RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$(dirname "$PERRY_BIN")}"
# Ad-hoc compiles must not link a per-app auto-optimized (feature-stripped)
# runtime: the diagnostics this gate reads (`[gc-copy-minor]`,
# `[gc-fromspace-scan ...]`) are ordinary env-gated eprintln!s (not behind
# the `diagnostics` cargo feature -- that name collision is with Node's
# `diagnostics_channel` support, a different thing), but auto-optimize can
# still relink against a differently-built stdlib/runtime pair than the one
# just built. Pin it explicitly rather than rely on that being harmless.
export PERRY_NO_AUTO_OPTIMIZE=1

FIXTURE="$REPO_ROOT/scripts/fixtures/gc_parse_churn_layout_state.ts"
if [[ ! -f "$FIXTURE" ]]; then
  echo "FAIL: fixture not found at $FIXTURE" >&2
  exit 1
fi

# Keep the gate's expected record count in sync with the fixture by reading
# it out, rather than hand-copying the number into this script (and letting
# it silently drift the next time someone retunes the fixture).
RECORDS="$(grep -oE '^const RECORDS = [0-9]+;' "$FIXTURE" | grep -oE '[0-9]+')"
if [[ -z "$RECORDS" ]]; then
  echo "FAIL: could not read 'const RECORDS = N;' out of $FIXTURE" >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "== compiling $FIXTURE (PERRY_GC_MOVING_LOOP_POLLS=1, records=$RECORDS) =="
# PERRY_GC_MOVING_LOOP_POLLS is a COMPILE-TIME gate as well as a runtime one
# (perry-codegen's moving_safepoint_polls_enabled() decides whether loop
# back-edge polls are even emitted) -- omitting it here would run the fixture
# below at safepoints that were never emitted, which the moving collector
# would then never reach without falling back to a conservative-stack-scan
# collection. A conservative-scan cycle is ineligible for the copying path
# (`CopiedMinorFallbackReason::ConservativeStack`), so it would run non-moving
# full mark-sweeps for the whole churn and never trigger the from-space scan
# at all -- the liveness check below exists exactly to catch that shape.
PERRY_GC_MOVING_LOOP_POLLS=1 "$PERRY_BIN" compile "$FIXTURE" -o "$WORK/fixture" >/dev/null

echo "== running under PERRY_JSON_TAPE=0 + PERRY_GC_FROMSPACE_SCAN_ABORT=1 =="
set +e
PERRY_GC_MOVING_LOOP_POLLS=1 \
PERRY_JSON_TAPE=0 \
PERRY_GC_FROMSPACE_SCAN_ABORT=1 \
PERRY_GC_DIAG=1 \
PERRY_GC_HEAP_LIMIT="${PERRY_GC_HEAP_LIMIT:-8}" \
  "$WORK/fixture" >"$WORK/stdout.txt" 2>"$WORK/stderr.txt"
RC=$?
set -e

echo "-- stdout --"
cat "$WORK/stdout.txt"
echo "-- stderr (tail) --"
tail -20 "$WORK/stderr.txt"
echo "-- exit code: $RC --"

python3 "$REPO_ROOT/scripts/gc_parse_churn_layout_check.py" \
  --exit-code "$RC" \
  --stdout "$WORK/stdout.txt" \
  --stderr "$WORK/stderr.txt" \
  --records "$RECORDS"
CHECK_RC=$?

exit "$CHECK_RC"
