#!/bin/bash
# Regression (fix/semver-module-token-build, #5358): a compiled CJS package
# whose module-level DESTRUCTURING require sits at the BOTTOM of the file — the
# canonical "require at the end for cyclic deps" pattern used by semver's
# classes/comparator.js (`const { safeRe: re, t } = require('../internal/re')`)
# — must still bind the destructured leaves into module scope so a class/method
# body lowered EARLIER in the file resolves them to the real module slot.
#
# Pre-fix: the module-level pre-registration pass only hoisted simple-ident
# bindings (`const x = ...`). A bottom `const { src, t } = require('./re')` was
# skipped, so the class constructor above it read `src`/`t` as undefined
# implicit globals (`src is not defined` / undefined). semver's subset.js builds
# `new Comparator('>=0.0.0-0')` at module-init time, which threw
# `TypeError: Invalid comparator: >=0.0.0-0`, blocking native-compile of semver.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi
if ! command -v cc >/dev/null 2>&1; then
  echo "SKIP: cc not available"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

PKG="$TMPDIR/node_modules/pk"
mkdir -p "$PKG"

# re.js — a module that builds up a table via a module-level closure (mirrors
# semver/internal/re.js `createToken`).
cat > "$PKG/re.js" << 'EOF'
'use strict'
exports = module.exports = {}
const src = exports.src = []
const t = exports.t = {}
let R = 0
const createToken = (name, value) => { const i = R++; t[name] = i; src[i] = value }
createToken('A', 'aaa')
createToken('B', 'bbb')
createToken('C', `${src[t.A]}-${src[t.B]}`)
EOF

# cons.js — the kept pattern: a class whose ctor reads `src`/`t`, with the
# DESTRUCTURING require at the BOTTOM of the file (cyclic-dep style).
cat > "$PKG/cons.js" << 'EOF'
'use strict'
class Thing {
  constructor(x) {
    if (!src || !t) throw new Error('bindings undefined for ' + x)
    this.v = src[t.C] + ':' + x
    if (!src[t.C]) throw new Error('Invalid: src[C] undefined for ' + x)
  }
}
module.exports = Thing
const { src, t } = require('./re.js')
EOF

# subset.js — constructs the class at MODULE-INIT time (mirrors semver
# ranges/subset.js `const minimumVersion = [new Comparator('>=0.0.0')]`).
cat > "$PKG/subset.js" << 'EOF'
'use strict'
const Thing = require('./cons.js')
const minimum = [new Thing('top-level')]
module.exports = function () { return minimum[0].v }
EOF

cat > "$PKG/index.js" << 'EOF'
'use strict'
const subset = require('./subset.js')
module.exports = function () { return subset() }
EOF

cat > "$PKG/package.json" << 'EOF'
{ "name":"pk","version":"1.0.0","main":"index.js" }
EOF

cat > "$TMPDIR/package.json" << 'EOF'
{ "type":"module","private":true,"perry":{"compilePackages":["*"],"allow":{"compilePackages":["*"]}} }
EOF

cat > "$TMPDIR/main.ts" << 'EOF'
import pk from "pk";
console.log(pk());
EOF

cd "$TMPDIR"
COMPILE_OUTPUT=$(PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" main.ts -o test_bin --no-cache 2>&1) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
}

RUN_OUTPUT=$(./test_bin 2>&1)
EXPECTED="aaa-bbb:top-level"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: bottom-of-file destructuring require not hoisted into module scope"
echo "Expected: $EXPECTED"
echo "Got:      $RUN_OUTPUT"
exit 1
