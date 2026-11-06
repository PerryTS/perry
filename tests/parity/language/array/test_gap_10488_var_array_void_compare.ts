// #10488: `arr[i] === void 0` (and other undefined-valued comparisons)
// against an out-of-bounds/hole read of a `var`-declared number-literal
// array always compiled `false` (and `!==` always `true`).
//
// A hoisted `var` lowers to TWO HIR `Let`s sharing one LocalId: a body-entry
// predefine (`Any = undefined`) and the real declaration (`Array(Number) =
// [0]`, say). `lower_let` (crates/perry-codegen/src/stmt/let_stmt.rs)
// refreshes `ctx.proven_local_types` on every `Let`, but a redeclaration
// (the second `Let`, since the predefine already allocated the slot) took
// an early return that never touched `ctx.local_types`. That left
// `is_numeric_expr` (which consults `proven_local_types`) and
// `expr_may_return_boxed_value_from_raw_f64_fallback`/`static_type_of`
// (which read the now-stale `local_types`, still `Any`) disagreeing about
// the very same local: the fallback-hazard guard never fired, so a
// strict-equality compare against the array element compiled to a bare
// `fcmp` — which can't represent the NaN-boxed `undefined` tag a hole or
// out-of-bounds read actually produces.

function varVoid(): boolean {
  var arr = [0];
  return arr[1] === void 0;
}
function varNe(): boolean {
  var arr = [0];
  return arr[1] !== void 0;
}
function varUndefVar(): boolean {
  var arr = [0];
  var u;
  return arr[1] === u;
}
function varTwoReads(): boolean {
  var arr = [0];
  return arr[1] === arr[2];
}
function varHole(): boolean {
  var arr = [0, 1];
  arr.length = 5;
  return arr[3] === void 0;
}
function varNegIdx(): boolean {
  var arr = [0];
  return arr[-1] === void 0;
}
function varPropCompare(): boolean {
  var arr = [0];
  var o: any = {};
  return arr[1] === o.v;
}
function varParamIdx(i: number): boolean {
  var arr = [0];
  return arr[i] === void 0;
}
// Controls: already correct in Perry before this fix; must stay correct.
function varUndefined(): boolean {
  var arr = [0];
  return arr[1] === undefined;
}
function letVoid(): boolean {
  let arr = [0];
  return arr[1] === void 0;
}
function varInBoundsVoid(): boolean {
  var arr = [0];
  return arr[0] === void 0;
}

var NUMERALS = "0123456789abcdef";
// decimal.js convertBase('255', 10, 16) (decimal.mjs:2608), verbatim.
function convertBase(str: string, baseIn: number, baseOut: number) {
  var j, arr = [0], arrL, i = 0, strL = str.length;
  for (; i < strL; ) {
    for (arrL = arr.length; arrL--; ) arr[arrL] *= baseIn;
    arr[0] += NUMERALS.indexOf(str.charAt(i++));
    for (j = 0; j < arr.length; j++) {
      if (arr[j] > baseOut - 1) {
        if (arr[j + 1] === void 0) arr[j + 1] = 0;
        arr[j + 1] += (arr[j] / baseOut) | 0;
        arr[j] %= baseOut;
      }
    }
  }
  return arr.reverse();
}

for (const f of [
  varVoid,
  varNe,
  varUndefVar,
  varTwoReads,
  varHole,
  varNegIdx,
  varPropCompare,
  varUndefined,
  letVoid,
  varInBoundsVoid,
]) {
  console.log(f.name, f());
}
console.log("varParamIdx", varParamIdx(1));
console.log("convertBase", JSON.stringify(convertBase("255", 10, 16)));
