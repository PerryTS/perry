#!/bin/bash
# Regression (#6604): a class EXPRESSION capturing an enclosing-function var
# assigned AFTER the class in source order — semver's shape in every bundled
# class file:
#
#   var Comparator = class _Comparator {
#     constructor(comp, options) { options = parseOptions(options); ... }
#   };
#   module.exports = Comparator;
#   var parseOptions = require_parse_options();   // assigned AFTER the class
#
# Correct JS: the ctor closes over the live binding; by the time anything
# constructs a Comparator the wrapper has completed and the binding holds the
# function. Pre-fix, the #6037/#6052 end-of-body capture-refresh machinery
# scanned only `ast::Decl::Class` DECLARATION statements, so no refresh was
# ever emitted for the expression shape: DYNAMIC construction of the escaped
# class value replayed the stale declaration-time snapshot (captured var =
# undefined forever) and threw "TypeError: value is not a function" at
# pi-native init (wall #2 of the pi coding-agent bring-up, behind #6593).
#
# Covers: var/let/const-assigned named and anonymous class expressions,
# argument-position class expressions, the esbuild `__commonJS` semver layout,
# captured-var reassignment tracking, and multi-evaluation factory isolation
# for the normal (assigned-before-class) shape.

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

cat > "$TMPDIR/main.js" << 'EOF'
// Shape 1: var-assigned NAMED class expression, captured var assigned after.
var wrapVar = function () {
  var C = class _C {
    constructor(x) { this.v = helper(x); }
  };
  var out = { K: C };
  var helper = function (s) { return "var:" + s; };
  return out;
};
console.log(new (wrapVar().K)("a").v);

// Shape 2: let / const anonymous class expressions.
var wrapLet = function () {
  let C = class {
    constructor(x) { this.v = helperL(x); }
  };
  var box = { K: C };
  var helperL = function (s) { return "let:" + s; };
  return box;
};
var wrapConst = function () {
  const C = class {
    constructor(x) { this.v = helperC(x); }
  };
  var box = { K: C };
  var helperC = function (s) { return "const:" + s; };
  return box;
};
console.log(new (wrapLet().K)("b").v);
console.log(new (wrapConst().K)("c").v);

// Shape 3: class expression in ARGUMENT position (no binding statement).
var registry = {};
var register = function (name, cls) { registry[name] = cls; };
var wrapArg = function () {
  register("K", class {
    constructor(x) { this.v = helperA(x); }
  });
  var helperA = function (s) { return "arg:" + s; };
};
wrapArg();
console.log(new registry.K("d").v);

// Shape 4: the esbuild __commonJS semver comparator.js layout.
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[Object.keys(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};
var require_parse_options = __commonJS({
  "parse-options.js"(exports, module) {
    "use strict";
    var emptyOpts = Object.freeze({});
    module.exports = (options) =>
      options && typeof options === "object" ? options : emptyOpts;
  },
});
var require_comparator = __commonJS({
  "comparator.js"(exports, module) {
    "use strict";
    var ANY = Symbol("SemVer ANY");
    var Comparator = class _Comparator {
      static get ANY() { return ANY; }
      constructor(comp, options) {
        options = parseOptions(options);
        if (comp instanceof _Comparator) { comp = comp.value; }
        this.loose = !!options.loose;
        this.value = String(comp);
      }
      toString() { return this.value; }
    };
    module.exports = Comparator;
    var parseOptions = require_parse_options();
  },
});
var Comparator = require_comparator();
var minimum = [new Comparator(">=0.0.0-0")];
var c2 = new Comparator(">=1.2.3", { loose: true });
console.log(String(minimum[0]), c2.value, c2.loose);

// Refresh must track REASSIGNMENT of a captured var, not just its init.
var wrapReassign = function () {
  var C = class {
    constructor() { this.v = tag(); }
  };
  var out = { K: C };
  var tag = function () { return "first"; };
  tag = function () { return "second"; };
  return out;
};
console.log(new (wrapReassign().K)().v);

// Per-evaluation isolation of the normal (assigned-before-class) shape.
var mk = function (t) {
  var prefix = "p" + t;
  var C = class {
    constructor() { this.v = t + ":" + prefix; }
  };
  return C;
};
var A = mk("x");
var B = mk("y");
console.log(new A().v, new B().v);
EOF

cd "$TMPDIR"
COMPILE_OUTPUT=$(PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile main.js -o test_bin --no-cache 2>&1) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
}

RUN_OUTPUT=$(./test_bin 2>&1)
EXPECTED="var:a
let:b
const:c
arg:d
>=0.0.0-0 >=1.2.3 true
second
x:px y:py"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: class-expression capture snapshot stale (vars assigned after the class)"
echo "Expected:"
echo "$EXPECTED"
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
