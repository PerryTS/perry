#!/usr/bin/env bash
set -euo pipefail

binary=$(realpath "${1:?pass the compiled fixture executable}")
fixture_dir=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$fixture_dir/../../.." && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT
cd "$repo_root"

PERRY_METHOD_SITE_STATS=1 PERRY_GC_FORCE_EVACUATE=1 \
  PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_DIAG=1 "$binary" \
  > "$tmp_dir/control.out" 2> "$tmp_dir/control.err"
A2_START_WORKER=1 PERRY_METHOD_SITE_STATS=1 PERRY_GC_FORCE_EVACUATE=1 \
  PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_DIAG=1 "$binary" \
  > "$tmp_dir/worker.out" 2> "$tmp_dir/worker.err"
cmp "$fixture_dir/expected.txt" "$tmp_dir/control.out"
cmp "$fixture_dir/expected-worker.txt" "$tmp_dir/worker.out"

sum_counter() {
  sed -n "s/.*$2=\([0-9][0-9]*\).*/\1/p" "$1" |
    awk '{ sum += $1 } END { print sum + 0 }'
}
control_hits=$(sum_counter "$tmp_dir/control.err" class_read_hits)
worker_hits=$(sum_counter "$tmp_dir/worker.err" class_read_hits)
control_rewrites=$(sum_counter "$tmp_dir/control.err" class_read_root_rewrites)
control_copied=$(sum_counter "$tmp_dir/control.err" copied_objects)
worker_copied=$(sum_counter "$tmp_dir/worker.err" copied_objects)
test "$worker_hits" -gt 0
test "$control_hits" -gt "$worker_hits"
test "$control_rewrites" -gt 0
test "$control_copied" -gt 0
test "$worker_copied" -gt 0
printf 'class_read_hits control=%s worker=%s; root_rewrites=%s; copied_objects control=%s worker=%s\n' \
  "$control_hits" "$worker_hits" "$control_rewrites" "$control_copied" "$worker_copied"
