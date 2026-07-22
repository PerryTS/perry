#!/bin/bash
# Regression for the wide-object quadratic (#6743 family): repeated
# `Object.defineProperty` and `k in obj` on an object past the keys-index
# threshold were O(N) per call (three linear scans in the define flow + one in
# the `in` operator's own-key walk) — O(N²) total. Webpack/Babel CJS re-export
# modules (`if (k in exports) …; Object.defineProperty(exports, k, {get})` per
# key) made a single @babel/types module take ~292ms vs node's 3ms, the
# dominant cost of pi's module init under perry.
#
# The fix answers own-key presence through the same authoritative sidecar the
# [[Set]] append path maintains. This test pins CORRECTNESS of that authority
# model (differential vs node) on the shapes it could get wrong:
#   - dedup: redefining an existing key on a wide object must not duplicate it
#   - attrs retention: redefine keeps omitted attributes of the existing prop
#   - SSO keys: short (<=5 byte) keys stored inline must be seen by the index
#   - delete-then-redefine: a shrunken keys array must invalidate the index
#   - non-extensible: define of a NEW key still throws after many defines
#   - `in`: present/absent/deleted answers on a wide object
# Plus a scaling sanity check: 2400 getter-defines must run well under the
# quadratic regime's time.

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
const N = 300; // past the keys-index threshold (32)

// 1. Build a wide object via defineProperty getters (the Babel re-export shape),
//    with a mix of long and SSO-short (<=5 byte) keys.
const t: any = {};
Object.defineProperty(t, "__esModule", { value: true });
for (let i = 0; i < N; i++) {
  const k = i % 3 === 0 ? "k" + i : "longKey_" + i; // "k7" etc. are SSO-short
  if (k in t) continue;
  Object.defineProperty(t, k, { enumerable: true, configurable: true, get: () => i });
}
console.log("keys", Object.keys(t).length, "sample", t.k3, t.longKey_1);

// 2. Dedup: redefining every existing key must not grow the key set.
for (let i = 0; i < N; i++) {
  const k = i % 3 === 0 ? "k" + i : "longKey_" + i;
  Object.defineProperty(t, k, { enumerable: true, configurable: true, get: () => i * 2 });
}
console.log("after-redef keys", Object.keys(t).length, "val", t.k3, t.longKey_1);

// 3. Attribute retention on redefine (omitted attrs keep current values).
const w: any = {};
for (let i = 0; i < 50; i++) w["p" + i] = i;
Object.defineProperty(w, "p10", { value: 99, writable: false, enumerable: true, configurable: true });
Object.defineProperty(w, "p10", { value: 100 }); // omits attrs -> retained
const d = Object.getOwnPropertyDescriptor(w, "p10")!;
console.log("retention", d.value, d.writable, d.enumerable, d.configurable);

// 4. delete-then-redefine: shrink must not leave a stale index answer.
delete t.longKey_1;
console.log("deleted", "longKey_1" in t, Object.keys(t).length);
Object.defineProperty(t, "longKey_1", { enumerable: true, configurable: true, get: () => -1 });
console.log("readded", "longKey_1" in t, t.longKey_1, Object.keys(t).length);

// 4b. Redefining a NON-configurable accessor must throw (invariants still
//     enforced through the sidecar presence answer).
Object.defineProperty(t, "lockedKey", { enumerable: true, get: () => 7 });
let lockedThrew = false;
try { Object.defineProperty(t, "lockedKey", { get: () => 8 }); } catch { lockedThrew = true; }
console.log("nonconfigurable-redef-throws", lockedThrew, t.lockedKey);

// 5. Non-extensible wide object: NEW key throws, existing configurable redefine ok.
Object.preventExtensions(t);
let threw = false;
try { Object.defineProperty(t, "brandNew", { value: 1 }); } catch { threw = true; }
console.log("nonextensible-new-throws", threw);

// 6. `in` answers across the wide object.
console.log("in-checks", "k3" in t, "longKey_2" in t, "absent_xyz" in t, "__esModule" in t);

// 7. Scaling sanity: 2400 getter defines (was ~527ms quadratic; linear is ~100ms).
const big: any = {};
const t0 = Date.now();
for (let i = 0; i < 2400; i++) Object.defineProperty(big, "g" + i, { enumerable: true, get: () => i });
const dt = Date.now() - t0;
console.log("scale-2400 keys", Object.keys(big).length, "fast", dt < 400);
EOF

cd "$TMPDIR"
"${COMPILE_ENV[@]}" "$PERRY" compile main.ts --output test_bin --no-cache >/dev/null 2>&1
PERRY_OUT=$(./test_bin 2>&1)
NODE_OUT=$(node main.ts 2>&1)

if [ "$PERRY_OUT" = "$NODE_OUT" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: perry output diverged from node (wide-object define/in)"
echo "--- node ---"; echo "$NODE_OUT"
echo "--- perry ---"; echo "$PERRY_OUT"
diff <(echo "$NODE_OUT") <(echo "$PERRY_OUT") || true
exit 1
