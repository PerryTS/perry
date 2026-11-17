#!/usr/bin/env bash
# The realm's link-time hooks, in the linked program, on this host.
#
# perry-runtime defines `perry_realm_core_entry`, `perry_realm_complete` and
# `perry_realm_family_phase` weak; the entry module of a program compiled with
# PERRY_REALM_TWO_PHASE=1 defines them strong. This compiles one program both
# ways through the real pipeline and checks, for the object format of this
# host (ELF, Mach-O):
#   * both binaries link, run and print the same output;
#   * the default program does not link the two-phase path at all;
#   * the two-phase program does.
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
if ! cmp -s "$work/realm-off.out" "$work/realm-on.out"; then
  diff "$work/realm-off.out" "$work/realm-on.out" || true
  echo "realm two-phase link check: the two arms print different output" >&2
  exit 1
fi
[ -s "$work/realm-off.out" ] || { echo "realm two-phase link check: no output" >&2; exit 1; }
two_phase='js_get_global_core|js_realm_complete|js_realm_two_phase|js_realm_family_phase|populate_realm_phase|populate_realm_core|populate_realm_rest'
if grep -E "$two_phase" "$work/realm-off.nm"; then
  echo "realm two-phase link check: a program compiled without the switch links the two-phase path (above)" >&2
  exit 1
fi
for symbol in js_get_global_core js_realm_complete js_realm_family_phase populate_realm_phase; do
  grep -q "$symbol" "$work/realm-on.nm" || {
    echo "realm two-phase link check: the two-phase program does not link $symbol" >&2
    exit 1
  }
done
echo "realm two-phase link check: ok ($(wc -l < "$work/realm-off.out" | tr -d ' ') lines equal in both arms; the default program links none of the two-phase path)"
