#!/bin/bash
# Run against separately pinned base/head instrumented binaries.
set -euo pipefail
ulimit -c 0
binary=$(readlink -f "${1:?binary}")
out=${2:?output directory}
arm=${3:?base or head}
mkdir -p "$out"
out=$(readlink -f "$out")
scripts=$(cd "$(dirname "$0")" && pwd)
: > "$out/results.tsv"
for watch in 0 1; do
  set +e
  env PERRY_SNAP_NOPROTECT=1 PERRY_SNAP22_WATCH=$watch PERRY_SNAP_CENSUS="$out/report-$watch" \
    timeout 120 python3 "$scripts/trace.py" "$out/native-$watch.jsonl" -- \
    setarch -R "$binary" > "$out/stdout-$watch" 2> "$out/stderr-$watch"
  rc=$?
  set -e
  printf '%s\t%s\n' "$watch" "$rc" >> "$out/results.tsv"
done
python3 "$scripts/check_protection.py" "$out" "$arm"
