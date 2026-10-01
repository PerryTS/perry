#!/usr/bin/env bash
# A relinked prototype on an instance chain: every read must answer from the
# new chain (expected.txt is node 26.5.1's output). Both site kinds must have
# primed before the relinks, or the fixture proves nothing about their facts.
set -euo pipefail
binary=$(realpath "${1:?pass the compiled fixture executable}")
fixture_dir=$(cd "$(dirname "$0")" && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT
sum_counter() {
  awk -v key="$2" 'index($0,key"="){split($0,a,key"="); split(a[2],b,/[^0-9]/); sum+=b[1]} END{print sum+0}' "$1"
}
for n in 40 41; do
  PERRY_METHOD_SITE_STATS=1 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 "$binary" "$n" > "$tmp_dir/out" 2> "$tmp_dir/err"
  cmp "$fixture_dir/expected.txt" "$tmp_dir/out"
  class_primes=$(sum_counter "$tmp_dir/err" class_read_primes)
  holder=$(sum_counter "$tmp_dir/err" read_holder_primes)
  test "$class_primes" -gt 0
  test "$holder" -gt 0
done
printf 'class_read_primes=%s read_holder_primes=%s\n' "$class_primes" "$holder"
