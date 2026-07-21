#!/bin/bash
# Regression: `+=` string append must re-pair a split UTF-16 surrogate the same
# way expression concat and Node do. Found while testing pi #6728: pi's
# `visibleWidth` strips ANSI by rebuilding a string one code unit at a time
# (`stripped += clean[i]`), which splits an emoji's surrogate pair across two
# `+=` appends. perry's append path used to copy the two lone 3-byte WTF-8
# surrogates verbatim instead of coalescing them into the astral char's 4-byte
# UTF-8 — so `[...s].length`/`codePointAt` disagreed with Node, and pi's TUI
# width invariant aborted on any emoji in a colored line.
#
# This is a differential test: perry's output must be byte-identical to Node's.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi
if ! command -v node >/dev/null 2>&1; then
  echo "SKIP: node not found (differential test needs node)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

COMPILE_ENV=()
if [ -f "$SCRIPT_DIR/../target/debug/libperry_runtime.a" ] || [ -f "$SCRIPT_DIR/../target/release/libperry_runtime.a" ]; then
  COMPILE_ENV=(env PERRY_NO_AUTO_OPTIMIZE=1)
fi

cat > "$TMPDIR/main.ts" << 'EOF'
function show(label: string, s: string): void {
  console.log(label + " len=" + s.length + " cp=" + [...s].length + " cp0=" + s.codePointAt(0) + " eq=");
}
const e = "a👋b";           // real astral char (UTF-8 in source)
const hi = e[1], lo = e[2]; // lone high + low surrogate from indexing

// 1. The bug: incremental += of the two halves must re-pair.
let Y = ""; Y += hi; Y += lo;
show("pluseq", Y);
console.log("pluseq-eq", Y === "👋", (hi + lo) === Y);

// 2. pi's ANSI-strip pattern: rebuild a string one code unit at a time.
let out = "";
for (let i = 0; i < e.length; i++) out += e[i];
console.log("rebuild", out === e, [...out].length);

// 3. Multiple emoji, some with a leading ASCII run (exercises both branches).
const t = "x😀y👋z🎉w";
let r = "";
for (let i = 0; i < t.length; i++) r += t[i];
console.log("multi", r === t, [...r].length, r.codePointAt(1));

// 4. A genuinely lone surrogate must STAY lone (no false merge).
let L = ""; L += hi; L += "Z";
console.log("lone", [...L].length, L.codePointAt(0), L.length);

// 5. ASCII fast path is unaffected (and stays correct at scale).
let A = "";
for (let i = 0; i < 500; i++) A += "aé"; // 1 ascii + 1 two-byte utf8
console.log("bulk", A.length, [...A].length);

// 6. Emoji halves split by an unrelated append in between (no false pair).
let M = ""; M += hi; M += "-"; M += lo;
console.log("split", [...M].length, M.codePointAt(0));
EOF

cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
PERRY_OUT=$(./test_bin 2>&1)
NODE_OUT=$(node main.ts 2>&1)

if [ "$PERRY_OUT" = "$NODE_OUT" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: perry output diverged from node (surrogate re-pairing on +=)"
echo "--- node ---"; echo "$NODE_OUT"
echo "--- perry ---"; echo "$PERRY_OUT"
diff <(echo "$NODE_OUT") <(echo "$PERRY_OUT") || true
exit 1
