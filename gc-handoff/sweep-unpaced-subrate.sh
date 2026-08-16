#!/bin/bash
# #7803 Step 1, corrected (ZOD-NOTES §34).
#
# RATE=1 + ALLOC_KB=0 makes the seed inert — every ordinal collects.
# RATE=0.1 + ALLOC_KB=0 keeps the pinned candidate set (loop_polls) and
# lets the seed select ~10% of them (~6,400 collections, same count as
# the paced RATE=1 config that fails ~40%).
#
# usage:
#   sweep-unpaced-subrate.sh <seed>
#   sweep-unpaced-subrate.sh <start> <end> <parallel>
set -u
BIN="${ZOD_BIN:-/tmp/zod}"
OUT="${ZOD_SWEEP_DIR:-/tmp/zod-sweep-r01}"
RATE="${ZOD_SWEEP_RATE:-0.1}"
TIMEOUT_SECS="${ZOD_SWEEP_TIMEOUT:-5400}"

run_one() {
  local s="$1"
  mkdir -p "$OUT"
  PERRY_GC_SCHEDULE_SEED=$s PERRY_GC_SCHEDULE_RATE=$RATE \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=0 \
  PERRY_UNCAUGHT_BACKTRACE=1 \
    timeout "$TIMEOUT_SECS" "$BIN" >"$OUT/o.$s" 2>"$OUT/e.$s"
  local rc=$?
  local sched
  sched=$(grep -h 'gc-schedule.*done' "$OUT/o.$s" "$OUT/e.$s" 2>/dev/null | tail -1)
  local err
  err=$(grep -m1 -h 'TypeError\|Error:' "$OUT/e.$s" 2>/dev/null | head -c 160)
  echo "seed $s exit=$rc | $sched | $err"
}

if [ $# -eq 1 ]; then
  run_one "$1"
elif [ $# -eq 3 ]; then
  start="$1"; end="$2"; par="$3"
  mkdir -p "$OUT"
  seq "$start" "$end" | xargs -P "$par" -I{} "$0" {} >>"$OUT/summary.log" 2>&1
  echo "DRIVER DONE seeds $start..$end rate=$RATE" >>"$OUT/summary.log"
else
  echo "usage: $0 <seed> | $0 <start> <end> <parallel>" >&2
  exit 2
fi
