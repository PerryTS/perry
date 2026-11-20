#!/usr/bin/env bash
# The realm's link-time hooks, in the linked program, on this host.
#
# perry-runtime defines `perry_realm_core_entry`, `perry_realm_complete` and
# `perry_realm_family_phase` weak; the entry module of a program compiled for a
# two-phase realm (the default; PERRY_REALM_TWO_PHASE=0 is off) defines them
# strong. This compiles one program both ways through the real pipeline and
# checks, for the object format of this host (ELF, Mach-O):
#   * both binaries link, run and print the same output;
#   * the program compiled with the switch off does not link the two-phase
#     path at all;
#   * the two-phase program does, with the switch unset as with it set;
#   * on ELF, the program compiled with the switch off has no tail section
#     (`.perry_tail`): nothing it links carries the section attribute.
#
# usage: realm_two_phase_link_check.sh <perry> <work-dir>
set -euo pipefail
perry="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
work="$2"
root="$(cd "$(dirname "$0")/.." && pwd)"
case="$root/tests/parity/node-suite/globals/test_gap_realm_two_phase_switch_on.ts"
nm_tool="${NM:-nm}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
compile() { # <arm> <env assignment>
  ( cd "$(dirname "$case")" &&
    env "$2" PERRY_KEEP_SYMBOLS=1 PERRY_NO_CACHE=1 "$perry" compile "$case" -o "$work/realm-$1" ) \
    > "$work/realm-$1.compile.log" 2>&1 || {
    cat "$work/realm-$1.compile.log"
    echo "realm two-phase link check: the $1 arm does not compile or link" >&2
    exit 1
  }
  "$work/realm-$1" > "$work/realm-$1.out"
  "$nm_tool" "$work/realm-$1" > "$work/realm-$1.nm"
}
compile off PERRY_REALM_TWO_PHASE=0
compile on PERRY_REALM_TWO_PHASE=1
compile default PERRY_NO_CACHE=1
for arm in on default; do
  if ! cmp -s "$work/realm-off.out" "$work/realm-$arm.out"; then
    diff "$work/realm-off.out" "$work/realm-$arm.out" || true
    echo "realm two-phase link check: the off arm and the $arm arm print different output" >&2
    exit 1
  fi
done
[ -s "$work/realm-off.out" ] || { echo "realm two-phase link check: no output" >&2; exit 1; }
two_phase='js_get_global_core|js_realm_complete|js_realm_two_phase|js_realm_family_phase|populate_realm_phase|populate_realm_core|populate_realm_rest'
if grep -E "$two_phase" "$work/realm-off.nm"; then
  echo "realm two-phase link check: a program compiled with the switch off links the two-phase path (above)" >&2
  exit 1
fi
if command -v readelf > /dev/null 2>&1 && readelf -SW "$work/realm-off" 2> /dev/null | grep -q '\.perry_tail'; then
  echo "realm two-phase link check: a program compiled with the switch off has a tail section (.perry_tail): something it links carries the section attribute" >&2
  exit 1
fi
for arm in on default; do
  for symbol in js_get_global_core js_realm_complete js_realm_family_phase populate_realm_phase; do
    grep -q "$symbol" "$work/realm-$arm.nm" || {
      echo "realm two-phase link check: the two-phase program ($arm arm) does not link $symbol" >&2
      exit 1
    }
  done
done
echo "realm two-phase link check: ok ($(wc -l < "$work/realm-off.out" | tr -d ' ') lines equal in the three arms; the program compiled with the switch off links none of the two-phase path and has no tail section)"
