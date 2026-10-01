#!/usr/bin/env bash
# The plain holder walk refuses every chain through a class prototype: its
# bare class identity does not pin the registry link. The class read entry
# must have primed (the fixture reached the class path), the plain holder
# entry must not have.
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
holder=$(sum_counter "$tmp_dir/err" read_holder_primes)
absent=$(sum_counter "$tmp_dir/err" read_absent_primes)
refused=$(sum_counter "$tmp_dir/err" read_holder_refused)
class_primes=$(sum_counter "$tmp_dir/err" class_read_primes)
test "$class_primes" -gt 0
test "$refused" -gt 0
test "$holder" -eq 0
test "$absent" -eq 0
printf 'read_holder_primes=%s read_absent_primes=%s (class-prototype chains refused=%s); class_read_primes=%s\n' "$holder" "$absent" "$refused" "$class_primes"
