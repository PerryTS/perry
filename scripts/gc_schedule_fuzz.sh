#!/usr/bin/env bash
#
# Sweep a compiled program across seeded GC schedules and report which seeds
# fail, with a copy-pasteable reproduce command for each.
#
#   ./scripts/gc_schedule_fuzz.sh <binary> [seed-count] [-- args...]
#
# WHY THIS EXISTS
#
# A rooting bug (#7154 family) is a value live but not rooted across a
# collection point. Whether it is *caught* depends on whether a collection lands
# inside that window, so the observed failure rate is a property of the GC
# schedule, not of the bug. Re-running one program 60 times re-runs one schedule
# 60 times: it explores almost nothing, and with zero failures in N runs the 95%
# upper bound on the true rate is only ~3/N (120 clean runs bound a 1.7% bug at
# 2.5% -- no evidence at all). Varying WHEN collections fire explores the actual
# bug space, and `PERRY_GC_SCHEDULE_SEED` makes each variation replayable.
#
# WHAT YOU MUST HAVE DONE ALREADY
#
# The seeded schedule can only select safepoints that exist. Loop back-edge
# polls are emitted only when the COMPILER ran with
# `PERRY_GC_MOVING_LOOP_POLLS` enabled (default ON since #7721; `=0` kills them); without them a
# compute-only program has no safepoints between event-loop turns and every seed
# behaves identically. This script warns when a run reports zero safepoints,
# because that is the shape of a sweep that cannot fail.
#
# Usage:
#   scripts/gc_schedule_fuzz.sh ./myprog                # seeds 1..40, rate 0.05
#   scripts/gc_schedule_fuzz.sh ./myprog 200            # seeds 1..200
#   RATE=0.3 scripts/gc_schedule_fuzz.sh ./myprog 200   # denser schedule
#   scripts/gc_schedule_fuzz.sh ./myprog 200 -- --help  # pass args to the target
#
# Environment:
#   RATE=<0..1>     PERRY_GC_SCHEDULE_RATE for every run (default 0.05).
#   FIRST_SEED=<n>  First seed of the sweep (default 1).
#   TIMEOUT=<secs>  Per-run wall-clock cap; a run that exceeds it is recorded as
#                   a `timeout` failure (default 300, 0 disables).
#   BASELINE=<n>    Also run <n> control runs with NO seed set, to measure the
#                   unamplified failure rate on the same binary (default 0).
#   KEEP=1          Keep the per-run logs instead of deleting the passing ones.
#   OUTDIR=<dir>    Where logs go (default a fresh mktemp -d).
#
# Exit status: 0 if every seed passed, 1 if any seed failed, 2 on misuse.

set -uo pipefail

if [[ $# -lt 1 ]]; then
  sed -n '2,45p' "$0" >&2
  exit 2
fi

BIN="$1"; shift
SEED_COUNT="${1:-40}"
if [[ "${1:-}" == "--" ]]; then
  SEED_COUNT=40
else
  shift || true
fi
TARGET_ARGS=()
if [[ "${1:-}" == "--" ]]; then
  shift
  TARGET_ARGS=("$@")
fi

# A non-integer or zero seed count would run the sweep loop zero times and then
# report "PASS: no seed failed" having proven nothing — the vacuous green this
# whole harness exists to avoid. Reject it at parse time.
if [[ ! "$SEED_COUNT" =~ ^[0-9]+$ || "$SEED_COUNT" -eq 0 ]]; then
  echo "gc_schedule_fuzz: seed-count must be a positive integer, got '$SEED_COUNT'" >&2
  exit 2
fi

if [[ ! -x "$BIN" ]]; then
  echo "gc_schedule_fuzz: no executable at '$BIN'" >&2
  exit 2
fi
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"

RATE="${RATE:-0.05}"
# The runtime CLAMPS an out-of-range rate, so `RATE=2` would run at 1 while every
# line this script prints -- including the reproduce command -- claimed 2. A
# reproduce command that does not describe the run it reproduces is worse than
# none, so reject rather than clamp.
if ! awk -v r="$RATE" 'BEGIN { exit !(r == r + 0 && r >= 0 && r <= 1) }' </dev/null 2>/dev/null; then
  echo "gc_schedule_fuzz: RATE must be a number in [0,1] (got '$RATE')." >&2
  echo "  The runtime clamps out-of-range values, so a sweep at RATE='$RATE'" >&2
  echo "  would report a density it did not run at." >&2
  exit 2
fi

# Allocation pacing is inherited from the environment and changes which polls
# the seed can select, so it belongs in every command this script prints. Pin it
# explicitly to the shipped default when the caller has not chosen one, so a
# reproduce command is complete rather than dependent on the reader's shell.
ALLOC_KB="${PERRY_GC_SCHEDULE_ALLOC_KB:-4}"
export PERRY_GC_SCHEDULE_ALLOC_KB="$ALLOC_KB"
FIRST_SEED="${FIRST_SEED:-1}"
TIMEOUT="${TIMEOUT:-300}"
BASELINE="${BASELINE:-0}"
OUTDIR="${OUTDIR:-$(mktemp -d)}"
mkdir -p "$OUTDIR"

# `timeout` is coreutils; macOS ships it as gtimeout when it ships it at all.
TIMEOUT_CMD=()
if [[ "$TIMEOUT" != "0" ]]; then
  if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_CMD=(timeout "$TIMEOUT")
  elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_CMD=(gtimeout "$TIMEOUT")
  else
    echo "gc_schedule_fuzz: no timeout(1) found; running without a per-run cap" >&2
  fi
fi

# Classify a failure by the first line that looks like a cause, so the summary
# groups seeds that found the same bug instead of listing 40 exit codes.
classify() {
  local log="$1" rc="$2"
  if [[ "$rc" == "124" || "$rc" == "137" ]]; then
    echo "timeout(${TIMEOUT}s)"
    return
  fi
  local line
  line="$(grep -m1 -E \
    'gc-fromspace-protect\] FAULT|PERRY PANIC|panicked at|TypeError|ReferenceError|RangeError|Uncaught|Segmentation fault|Abort trap|signal: ' \
    "$log" 2>/dev/null | head -c 160)"
  if [[ -n "$line" ]]; then
    echo "${line//$'\n'/ }"
  else
    echo "exit $rc"
  fi
}

run_once() {
  # $1 = log path; remaining env comes from the caller's exported vars.
  local log="$1"
  local rc=0
  ${TIMEOUT_CMD[@]+"${TIMEOUT_CMD[@]}"} "$BIN" ${TARGET_ARGS[@]+"${TARGET_ARGS[@]}"} >"$log" 2>&1 || rc=$?
  return $rc
}

echo "== gc_schedule_fuzz =="
echo "   binary : $BIN ${TARGET_ARGS[*]:-}"
echo "   seeds  : $FIRST_SEED..$((FIRST_SEED + SEED_COUNT - 1))  (rate=$RATE)"
echo "   logs   : $OUTDIR"
echo

# --- control arm -------------------------------------------------------------
baseline_failures=0
if [[ "$BASELINE" -gt 0 ]]; then
  echo "-- control: $BASELINE runs with NO seed (unamplified rate) --"
  for ((i = 1; i <= BASELINE; i++)); do
    log="$OUTDIR/baseline-$i.log"
    rc=0
    env -u PERRY_GC_SCHEDULE_SEED -u PERRY_GC_SCHEDULE_RATE \
      ${TIMEOUT_CMD[@]+"${TIMEOUT_CMD[@]}"} "$BIN" ${TARGET_ARGS[@]+"${TARGET_ARGS[@]}"} >"$log" 2>&1 || rc=$?
    if [[ $rc -ne 0 ]]; then
      baseline_failures=$((baseline_failures + 1))
      printf '   run %-4s FAIL  %s\n' "$i" "$(classify "$log" "$rc")"
    else
      [[ "${KEEP:-0}" == "1" ]] || rm -f "$log"
    fi
  done
  echo "   control: $baseline_failures/$BASELINE failed"
  echo
fi

# --- seeded arm --------------------------------------------------------------
declare -a FAILED_SEEDS=()
declare -a FAILED_CAUSES=()
passed=0
saw_safepoints=0
start_epoch=$(date +%s)

for ((n = 0; n < SEED_COUNT; n++)); do
  seed=$((FIRST_SEED + n))
  log="$OUTDIR/seed-$seed.log"
  rc=0
  PERRY_GC_SCHEDULE_SEED="$seed" PERRY_GC_SCHEDULE_RATE="$RATE" \
    PERRY_GC_SCHEDULE_ALLOC_KB="$ALLOC_KB" \
    run_once "$log" || rc=$?

  # Liveness, per CLAUDE.md's "a gate must assert its subject was live": a
  # sweep in which the schedule saw zero safepoints proves nothing at all.
  if grep -q '\[gc-schedule\] .*safepoints=[1-9]' "$log" 2>/dev/null; then
    saw_safepoints=1
  fi

  if [[ $rc -ne 0 ]]; then
    cause="$(classify "$log" "$rc")"
    FAILED_SEEDS+=("$seed")
    FAILED_CAUSES+=("$cause")
    printf '   seed %-8s FAIL  %s\n' "$seed" "$cause"
  else
    passed=$((passed + 1))
    [[ "${KEEP:-0}" == "1" ]] || rm -f "$log"
  fi
done

elapsed=$(( $(date +%s) - start_epoch ))

echo
echo "== summary =="
echo "   seeds run     : $SEED_COUNT"
echo "   passed        : $passed"
echo "   failed        : ${#FAILED_SEEDS[@]}"
if [[ "$SEED_COUNT" -gt 0 ]]; then
  echo "   failure rate  : $(awk -v f="${#FAILED_SEEDS[@]}" -v n="$SEED_COUNT" \
    'BEGIN { printf "%.1f%%", 100 * f / n }')"
fi
if [[ "$BASELINE" -gt 0 ]]; then
  echo "   control rate  : $(awk -v f="$baseline_failures" -v n="$BASELINE" \
    'BEGIN { printf "%.1f%%", 100 * f / n }') ($baseline_failures/$BASELINE, no seed)"
fi
echo "   wall clock    : ${elapsed}s ($(awk -v e="$elapsed" -v n="$SEED_COUNT" \
  'BEGIN { printf "%.1f", e / (n > 0 ? n : 1) }')s/run)"

if [[ "$saw_safepoints" -eq 0 ]]; then
  # A clean sweep that selected nothing proves nothing — refuse to call it a
  # PASS. Reporting success here is exactly the vacuous green this harness
  # exists to catch.
  echo
  echo "INCONCLUSIVE: no run reported a nonzero safepoint count."
  echo "   The seeded schedule had nothing to select, so a clean sweep here"
  echo "   means nothing. Check the target was not compiled with"
  echo "   PERRY_GC_MOVING_LOOP_POLLS=0 (polls are default ON since #7721)."
  exit 2
fi

if [[ ${#FAILED_SEEDS[@]} -eq 0 ]]; then
  echo
  echo "PASS: no seed failed ($SEED_COUNT seeds, safepoints exercised)."
  exit 0
fi

echo
echo "== reproduce =="
for i in "${!FAILED_SEEDS[@]}"; do
  seed="${FAILED_SEEDS[$i]}"
  echo "  # ${FAILED_CAUSES[$i]}"
  echo "  PERRY_GC_SCHEDULE_SEED=$seed PERRY_GC_SCHEDULE_RATE=$RATE \\"
  echo "  PERRY_GC_SCHEDULE_ALLOC_KB=$ALLOC_KB $BIN ${TARGET_ARGS[*]:-}"
  echo "  #   log: $OUTDIR/seed-$seed.log"
  echo "  #   for a precise fault site, add:"
  echo "  #     PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800"
  echo
done
exit 1
