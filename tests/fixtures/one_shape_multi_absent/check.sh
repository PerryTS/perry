#!/usr/bin/env bash
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
primes=$(sum_counter "$tmp_dir/err" read_absent_primes)
rewrites=$(sum_counter "$tmp_dir/err" read_holder_rewrites)
copied=$(sum_counter "$tmp_dir/err" copied_objects)
test "$primes" -gt 0
test "$primes" -lt 100
test "$rewrites" -gt 0
test "$copied" -gt 0
printf 'read_absent_primes=%s (<100 across repeated shapes); holder_rewrites=%s; copied_objects=%s\n' "$primes" "$rewrites" "$copied"
