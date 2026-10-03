#!/usr/bin/env bash
# An inherited-read entry's holder (data kind, depth 1..3, reachable only
# through the receiver's link) must be rooted and rewritten when a collection
# moves it. The values cannot tell: an unrooted entry's stale address fails
# its shape compare and the site re-primes, so the observable is the counters.
set -euo pipefail
binary=$(realpath "${1:?pass the compiled fixture executable}")
fixture_dir=$(cd "$(dirname "$0")" && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT
PERRY_METHOD_SITE_STATS=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_DIAG=1 "$binary" > "$tmp_dir/out" 2> "$tmp_dir/err"
cmp "$fixture_dir/expected.txt" "$tmp_dir/out"
sum_counter() {
  awk -v key="$2" 'index($0,key"="){split($0,a,key"="); split(a[2],b,/[^0-9]/); sum+=b[1]} END{print sum+0}' "$1"
}
primes=$(sum_counter "$tmp_dir/err" read_holder_primes)
rewrites=$(sum_counter "$tmp_dir/err" read_holder_rewrites)
copied=$(sum_counter "$tmp_dir/err" copied_objects)
test "$primes" -gt 0
test "$primes" -le 8
test "$rewrites" -gt 0
test "$copied" -gt 0
printf 'read_holder_primes=%s (<=8: moved holders do not re-prime); holder_rewrites=%s; copied_objects=%s\n' "$primes" "$rewrites" "$copied"
