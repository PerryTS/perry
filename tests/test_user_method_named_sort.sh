#!/usr/bin/env bash
set -euo pipefail

# A user-defined method named `sort` on a class instance — or a function
# re-exported as `sort` from an imported module (semver:
# `sort = (list) => list.sort(cmp)` called as `semver.sort(list)`) — must NOT
# be folded into the `Expr::ArraySort` array intrinsic. The fold mis-routed the
# single `list` argument into the comparator slot, so the runtime comparator
# validator saw the array (not a function) and threw "The comparison function
# must be either a function or undefined: ...". The HIR sort arms in
# `array_only_methods.rs` / `imported_array_methods.rs` now require the
# receiver to be a known array (not a class instance / imported binding) before
# folding, mirroring the existing `map`/`filter`/`join` guards.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/f.ts" <<'TS'
// A wrapper class whose `sort(list)` method internally calls Array.sort with
// a comparator — exactly semver's re-exported `sort`.
class Sorter {
  sort(list: any[]) {
    return list.sort((a: any, b: any) => (a < b ? -1 : a > b ? 1 : 0));
  }
}
const s = new Sorter();
console.log(JSON.stringify(s.sort(["1.2.0", "1.0.1", "1.0.0", "2.0.0"])));
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
EXPECT='["1.0.0","1.0.1","1.2.0","2.0.0"]'
if ! grep -qF "$EXPECT" <<<"$OUT"; then
    echo "FAIL: expected $EXPECT, got:"; echo "$OUT"; exit 1
fi
echo "PASS: user-method-named-sort is not folded to the array intrinsic"
