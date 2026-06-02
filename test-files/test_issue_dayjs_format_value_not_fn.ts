// Regression for the dayjs `format("YYYY-MM")` crash where calling the
// global `Array` builtin without `new` threw `TypeError: value is not a
// function`. dayjs's `padStart` utility (used by every `format()` for any
// 2-digit field like the "MM" in "YYYY-MM") does:
//
//   var padStart = function padStart(string, length, pad) {
//     var s = String(string);
//     if (!s || s.length >= length) return string;
//     return "" + Array(length + 1 - s.length).join(pad) + string;
//   };
//
// Pre-fix, perry lowered the bare `Array(...)` call to the unknown-ident
// sentinel (`Call { callee: GlobalGet(0), ... }`); the runtime closure-call
// dispatcher then validated the closure pointer, found it wasn't a real
// closure, and routed through `throw_not_callable` → `TypeError: value is
// not a function`. ES2015 §22.1.1 mandates that `Array(...)` and
// `new Array(...)` produce identical results, so we now route the bare-call
// shape through the same `Expr::New { class_name: "Array", ... }` HIR variant
// that `new Array(...)` already used.
//
// This standalone shape mirrors dayjs's padStart pattern: bare-call `Array`
// with a numeric length, then `.join(...)` against the resulting array.

// Bare `Array(n)` must return an array of length n (per spec, slots NaN-boxed
// undefined). Mirrors `padStart`'s `Array(length + 1 - s.length)`.
const a = Array(3);
console.log("a.length:", a.length);

// Bare `Array()` with no args returns `[]`.
const empty = Array();
console.log("empty.length:", empty.length);

// The full dayjs padStart shape, inline.
function padStart(s: any, length: number, pad: string): string {
  const str = String(s);
  if (!str || str.length >= length) return str;
  return "" + Array(length + 1 - str.length).join(pad) + str;
}

// dayjs uses this for every 2-digit field (MM, DD, HH, mm, ss).
// We only check that the call doesn't throw — perry's pre-existing
// `Array(n).join(pad)` semantics gap (#-tracked separately) means the
// padding bytes themselves may not match Node yet, but the throw is gone.
let threw = false;
try {
  padStart(8, 2, "0");
  padStart(2024, 4, "0");
} catch (_e) {
  threw = true;
}
console.log("threw:", threw);
