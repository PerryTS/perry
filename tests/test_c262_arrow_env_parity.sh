#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

run_case() {
    local name="$1"
    local expected="$2"
    local source="$TMPDIR/$name.js"
    local binary="$TMPDIR/$name.out"
    local log="$TMPDIR/$name.log"

    PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-cache "$source" -o "$binary" \
        >"$TMPDIR/$name.compile.log" 2>&1 || {
            echo "FAIL: $name compile failed"
            sed 's/^/    /' "$TMPDIR/$name.compile.log" | tail -80
            exit 1
        }

    "$binary" >"$log" 2>&1 || {
        echo "FAIL: $name program failed"
        sed 's/^/    /' "$log" | tail -80
        exit 1
    }

    if [[ "$(cat "$log")" != "$expected" ]]; then
        echo "FAIL: $name expected '$expected'"
        sed 's/^/    /' "$log" | tail -80
        exit 1
    fi
}

cat >"$TMPDIR/dflt-params-ref-self.js" <<'JS'
let ok = false;
try { ((x = x) => 1)(); } catch (e) { ok = e instanceof ReferenceError; }
console.log(ok ? "ok" : "bad");
JS

cat >"$TMPDIR/eval-var-scope-syntax-err.js" <<'JS'
let ok = false;
try { ((a = eval("var a = 42")) => 1)(); } catch (e) { ok = e instanceof SyntaxError; }
console.log(ok ? "ok" : "bad");
JS

cat >"$TMPDIR/lexical-super-call-from-within-constructor.js" <<'JS'
var count = 0;
class A { constructor() { count += 1; } }
class B extends A { constructor() { super(); this.af = _ => super(); } }
var b = new B();
let ok = false;
try { b.af(); } catch (e) { ok = e instanceof ReferenceError; }
console.log(ok + ":" + count);
JS

cat >"$TMPDIR/non-strict.js" <<'JS'
var af = _ => { foo = 1; };
af();
console.log(foo);
JS

cat >"$TMPDIR/scope-body-lex-distinct.js" <<'JS'
let ok = false;
try { (() => { let x; eval("var x;"); })(); } catch (e) { ok = e instanceof SyntaxError; }
console.log(ok ? "ok" : "bad");
JS

cat >"$TMPDIR/scope-param-rest-elem-var-open.js" <<'JS'
var x = "outside";
var probe1, probe2;
((
    _ = probe1 = function() { return x; },
    ...[__ = (eval("var x = \"inside\";"), probe2 = function() { return x; })]
  ) => {
})();
console.log(probe1() + ":" + probe2());
JS

cat >"$TMPDIR/scope-paramsbody-var-open.js" <<'JS'
var x = "outside";
var probeParams, probeBody;
((_ = probeParams = function() { return x; }) => {
  var x = "inside";
  probeBody = function() { return x; };
})();
console.log(probeParams() + ":" + probeBody());
JS

run_case dflt-params-ref-self ok
run_case eval-var-scope-syntax-err ok
run_case lexical-super-call-from-within-constructor true:2
run_case non-strict 1
run_case scope-body-lex-distinct ok
run_case scope-param-rest-elem-var-open inside:inside
run_case scope-paramsbody-var-open outside:inside

echo "PASS: c262 arrow environment parity"
