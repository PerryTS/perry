#!/usr/bin/env bash
# Decision 5 of RFC deferred collection, the ratchet-probe half: compile every
# GC ratchet probe once, run it once with PERRY_GC_VALVE_LEDGER set, and fail
# if any allocation-point valve fired or if any probe did not report
# (scripts/gc_valve_ledger_check.py). The gap-suite half runs in the gap-suite
# shards of test.yml.
#
# Usage: scripts/gc_valve_ratchet_probes.sh [path-to-perry]
set -euo pipefail

PERRY_BIN="${1:-target/release/perry}"
if [[ ! -x "$PERRY_BIN" ]]; then
  echo "FAIL: no perry binary at $PERRY_BIN" >&2
  exit 1
fi
PERRY_BIN="$(cd "$(dirname "$PERRY_BIN")" && pwd)/$(basename "$PERRY_BIN")"
export PERRY_RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$(dirname "$PERRY_BIN")}"
export PERRY_NO_AUTO_OPTIMIZE=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
LEDGER="$WORK/ledger.txt"

python3 "$ROOT/scripts/gc_valve_ledger_check.py" --self-test

count=0
for probe in "$ROOT"/benchmarks/gc_ratchet/probes/*.ts; do
  name="$(basename "$probe" .ts)"
  "$PERRY_BIN" compile "$probe" -o "$WORK/$name" > "$WORK/$name.compile.log" 2>&1 || {
    echo "FAIL: compiling $name" >&2
    tail -20 "$WORK/$name.compile.log" >&2
    exit 1
  }
  PERRY_GC_VALVE_LEDGER="$LEDGER" "$WORK/$name" > /dev/null
  count=$((count + 1))
done
echo "ran $count ratchet probes"
python3 "$ROOT/scripts/gc_valve_ledger_check.py" --ledger "$LEDGER" --expect-min "$count"
