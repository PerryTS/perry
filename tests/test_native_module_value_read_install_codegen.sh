#!/usr/bin/env bash
# Regression: reading a native-module callable export AS A VALUE
# (`const f = util.inherits`) and invoking it indirectly must dispatch to the
# real runtime impl. The indirect call resolves through the per-module
# NM_DISPATCH_REGISTRY, which is populated by `js_nm_install_<module>()`. The
# *direct* call form (`util.inherits(a, b)`) is statically lowered straight to
# the runtime extern and never touches that registry, so a module reached ONLY
# through the value-read path used to leave the registry empty — the indirect
# call silently resolved to `undefined`.
#
# Concretely: winston's `class Logger extends Transform` (readable-stream)
# relies on `require('inherits')(Transform, Duplex)` — an indirect
# `util.inherits` value-call — to wire the ES5 super-chain so the nested
# `Readable.call(this)` `if (!(this instanceof Readable))` guard takes the
# in-place branch and sets `this._readableState`. With the install skipped,
# the guard saw `false`, returned a discarded `new Readable()`, and
# `this._readableState.needReadable = true` threw on `null`.
#
# This test pins both halves:
#   1. RUNTIME: the value-read `inherits(Sub, Base)` registers the ES5 parent
#      edge so `new Sub() instanceof Base` is true and base ctor writes persist.
#   2. CODEGEN: the `PropertyGet { NativeModuleRef("util"), "inherits" }`
#      value-read emits `call void @js_nm_install_util()`.
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

SRC="$TMPDIR/nm_value_read_install.ts"
BIN="$TMPDIR/nm_value_read_install"
OBJ="$TMPDIR/nm_value_read_install.o"

cat >"$SRC" <<'TS'
import * as util from "util";

// Read the native-module callable export AS A VALUE, then invoke indirectly.
const inh: any = (util as any).inherits;

function Base(this: any) {
  if (!(this instanceof Base)) return new (Base as any)();
  this._state = { x: 1 };
}
function Sub(this: any) {
  (Base as any).call(this);
  this._state.y = 2; // mirrors readable-stream's post-super `this._x` writes
}
inh(Sub, Base);

const s: any = new (Sub as any)();

let failures = 0;
if (!(s instanceof Base)) {
  console.log("FAIL: value-read util.inherits did not register the ES5 parent edge");
  failures = failures + 1;
}
if (!s._state || s._state.x !== 1 || s._state.y !== 2) {
  console.log("FAIL: base ctor writes did not persist through the super-chain");
  failures = failures + 1;
}

if (failures !== 0) {
  throw new Error("native-module value-read inherits regression failed");
}
console.log("nm value-read inherits ok");
TS

"$PERRY" compile --no-cache --no-auto-optimize "$SRC" -o "$BIN" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$BIN" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "nm value-read inherits ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

(
    cd "$TMPDIR"
    "$PERRY" compile --no-cache --no-auto-optimize --trace llvm --no-link \
        "$SRC" -o "$OBJ" >"$TMPDIR/trace-compile.log" 2>&1
) || {
    echo "FAIL: trace compile failed"
    sed 's/^/    /' "$TMPDIR/trace-compile.log" | tail -80
    exit 1
}

TRACE_DIR="$TMPDIR/.perry-trace/llvm"
if [[ ! -d "$TRACE_DIR" ]]; then
    echo "FAIL: LLVM trace directory not found"
    exit 1
fi

if ! grep -R "call void @js_nm_install_util()" "$TRACE_DIR" >/dev/null; then
    echo "FAIL: expected js_nm_install_util() call on the native-module value-read path"
    exit 1
fi

echo "PASS: native-module value-read install codegen"
