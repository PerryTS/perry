#!/bin/bash
# Regression (fix/ajv-not-constructor): a compiled CJS package whose class is
# kept inside the cjs_wrap IIFE (#5251 hoist guard — the class body references
# the injected `exports`) must still surface its `exports.X = X` named export to
# `require('./code').X`. The HIR lowering for a function-nested `class X {}` now
# defines a scope-local binding shadowing the outer same-named re-export const,
# matching how a nested `function X(){}` already behaves.
#
# Pre-fix: `require('./code').Name` was `undefined` (the in-IIFE `exports.Name =
# Name` resolved `Name` to the circular module-scope `const Name = _cjs.Name`),
# so ajv's `new code_1.Name(...)` threw "undefined is not a constructor".
# The function variant in the identical shape always worked — this guards both.

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

# code.js — the kept-in-IIFE shape: `class Name` whose ctor reads the injected
# `exports`, then `exports.Name = Name`. The #5251 guard keeps the class inside
# the IIFE; the named export must still reach the module's external exports.
cat > "$PKG/code.js" << 'EOF'
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.Name = void 0;
exports.IDENTIFIER = /^[a-z]+$/i;
class Name { constructor(s){ if(!exports.IDENTIFIER.test(s)) throw new Error("bad"); this.str = s; } }
exports.Name = Name;
EOF

# func.js — the FUNCTION variant of the identical shape (the asymmetry to guard
# against re-breaking): a function kept in the IIFE exported the same way.
cat > "$PKG/func.js" << 'EOF'
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.Make = void 0;
exports.PREFIX = "v";
function Make(s){ this.tag = exports.PREFIX + s; }
exports.Make = Make;
EOF

# scope.js — cross-module consumer that constructs the kept-in-IIFE class at
# module-init time (mirrors ajv's scope.js `new code_1.Name("const")`).
cat > "$PKG/scope.js" << 'EOF'
"use strict";
const code_1 = require("./code");
const func_1 = require("./func");
exports.varKinds = { const: new code_1.Name("const") };
exports.made = new func_1.Make("x");
EOF

cat > "$PKG/index.js" << 'EOF'
"use strict";
const code_1 = require("./code");
const func_1 = require("./func");
const scope_1 = require("./scope");
module.exports = function(){
  return "Name=" + typeof code_1.Name
    + " Make=" + typeof func_1.Make
    + " kind=" + String(scope_1.varKinds.const)
    + " made=" + scope_1.made.tag;
};
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
# Matches `node main.ts`: the class & function both export as functions, and the
# cross-module `new code_1.Name(...)` / `new func_1.Make(...)` succeed.
EXPECTED="Name=function Make=function kind=[object Object] made=vx"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: kept-in-IIFE class named export not visible on require()"
echo "Expected: $EXPECTED"
echo "Got:      $RUN_OUTPUT"
exit 1
