#!/usr/bin/env bash
set -euo pipefail

# `String.prototype.replace(re, fn)` must fire the callback even when the regex
# needs lookahead/backreferences (which the `regex` crate can't compile, so the
# pattern is stashed in the fancy-regex FANCY_CACHE behind a never-match
# placeholder). The callback-replace path had no fancy-regex fallback, so the
# callback never fired and the input came back unchanged — get-intrinsic's
# `stringToPath` (`'%String.prototype.indexOf%'.replace(/…(?=…)…/g, fn)`)
# returned an empty parts list, breaking the whole call-bound→qs→Stripe chain.

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
// Lookahead alternative — needs fancy-regex.
function run(re: RegExp, s: string): string[] {
  const out: string[] = [];
  s.replace(re, (m: string) => { out.push(m); return m; });
  return out;
}
// `[a-z]+ | (?=\.)`: first match "ab", a zero-width lookahead at '.', "cd".
const a = run(/[a-z]+|(?=\.)/g, "ab.cd");
if (JSON.stringify(a) !== '["ab","cd"]') throw new Error("A: " + JSON.stringify(a));
// Pure zero-width lookahead repeated — must not infinite-loop.
const d = run(/(?=\.)/g, "a.b.c");
if (JSON.stringify(d) !== '["",""]') throw new Error("D: " + JSON.stringify(d));
// get-intrinsic's stringToPath regex.
const rePropName = /[^%.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|%$))/g;
const result: any[] = [];
"%String.prototype.indexOf%".replace(rePropName, function (m: any, num: any, q: any, sub: any) {
  result[result.length] = q ? sub : (num || m);
  return m;
} as any);
if (JSON.stringify(result) !== '["String","prototype","indexOf"]') {
  throw new Error("rePropName: " + JSON.stringify(result));
}
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: regex replace callback with lookahead (fancy-regex fallback)"
