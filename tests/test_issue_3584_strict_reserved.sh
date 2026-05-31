#!/bin/bash
# Regression for #3584: script source-goal reserved words, strict assignment
# throws, and object-literal reserved-word accessors.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

cat > "$TMPDIR/non_strict_public.js" << 'EOF'
function testcase() {
  "USE STRICT";
  var public = 1;
  console.log(public);
}
testcase();
EOF

cat > "$TMPDIR/await_script.js" << 'EOF'
var await = 7;
console.log(await);
EOF

cat > "$TMPDIR/strict_public.js" << 'EOF'
"use strict";
var public = 1;
EOF

cat > "$TMPDIR/const_assignment.js" << 'EOF'
const test262const = 3;
try {
  test262const = 4;
  console.log("no-throw");
} catch (e) {
  console.log(e.name);
}
EOF

cat > "$TMPDIR/strict_unresolvable.js" << 'EOF'
"use strict";
try {
  x3584 = 1;
  console.log("no-throw");
} catch (e) {
  console.log(e.name);
}
EOF

cat > "$TMPDIR/accessor_reserved.js" << 'EOF'
var test = "unset";
var tokenCodes = {
  set await(value) {
    test = "await";
  },
  get await() {
    return test;
  }
};
tokenCodes.await = 0;
console.log(tokenCodes.await);
EOF

cd "$TMPDIR"

"$PERRY" compile non_strict_public.js --output non_strict_public >/dev/null 2>&1
[ "$(./non_strict_public 2>&1)" = "1" ]

"$PERRY" compile await_script.js --output await_script >/dev/null 2>&1
[ "$(./await_script 2>&1)" = "7" ]

if "$PERRY" compile strict_public.js --output strict_public >/tmp/issue_3584_strict.log 2>&1; then
  echo "FAIL: strict reserved word binding compiled"
  exit 1
fi

"$PERRY" compile const_assignment.js --output const_assignment >/dev/null 2>&1
[ "$(./const_assignment 2>&1)" = "TypeError" ]

"$PERRY" compile strict_unresolvable.js --output strict_unresolvable >/dev/null 2>&1
[ "$(./strict_unresolvable 2>&1)" = "ReferenceError" ]

"$PERRY" compile accessor_reserved.js --output accessor_reserved >/dev/null 2>&1
[ "$(./accessor_reserved 2>&1)" = "await" ]

echo "PASS"
