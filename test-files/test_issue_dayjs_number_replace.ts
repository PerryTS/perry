// Regression for the dayjs `format("YYYY-MM")` crash where the inner
// regex-replace callback observed its captured `i` (the UTC offset zone
// string, e.g. "+02:00") as a `number` instead of a `string`, then
// threw `TypeError: (number).replace is not a function` on the
// `i.replace(":","")` zoneStr fallback.
//
// Two cooperating bugs were at play:
//
// 1. `Array(n).join(sep)` returned "[object Object]" for each TAG_HOLE
//    slot — `js_array_alloc_with_length` initialises slots to TAG_HOLE
//    but `js_array_join` didn't recognise the sentinel, so the catch-all
//    branch emitted the placeholder string instead of the spec's empty
//    string. `padStart`-style helpers like dayjs's `m(t,e,n)` build the
//    UTC offset via `Array(e+1-r.length).join(n)` and silently
//    corrupted `b.z(this)` (the captured `i`).
//
// 2. The codegen module-wide `boxed_vars` set missed every
//    `Stmt::PreallocateBoxes` inside a closure that was registered via
//    `Expr::RegisterFunctionPrototypeMethod` (the lowering for
//    `Constructor.prototype.method = function () {...}`). The walker in
//    `collect_nested_closure_boxed_vars_in_expr` had a catch-all
//    `_ => {}` and so never descended into the format closure's body.
//    Consequently the inner replace callback's
//    `boxed_vars.contains(150 /* outer `i` */) == false` branch read
//    the capture slot as a raw f64 — which is the box pointer's bit
//    pattern — and `typeof` reported it as a tiny denormal `number`.
//
// This test exercises both shapes with plain TypeScript so it doesn't
// require dayjs to be installed.

// --- 1. Array(n).join(sep) sparse-hole semantics ---

console.log("Array(3).join('0'):", Array(3).join("0")); // expect "00"
console.log("Array(2).join('-'):", Array(2).join("-")); // expect "-"
console.log("Array(1).join('x'):", Array(1).join("x")); // expect ""
console.log("Array(0).join(','):", Array(0).join(",")); // expect ""

// padStart-style helper (the exact shape dayjs uses).
function pad(t: number | string, width: number, ch: string): string {
  const r = String(t);
  return !r || r.length >= width ? r : "" + Array(width + 1 - r.length).join(ch) + t;
}
console.log("pad(7,2,'0'):", pad(7, 2, "0")); // expect "07"
console.log("pad(123,4,'0'):", pad(123, 4, "0")); // expect "0123"

// --- 2. boxed-capture through Constructor.prototype.method ---
// Mirror dayjs's UMD shape: an inner function's prototype gets a
// `format` method whose body holds many `var` bindings (which the HIR
// emits as `Stmt::PreallocateBoxes`) and creates an inner replace-
// callback that captures `i` and calls `i.replace(":","")`.

function Outer(this: any) {}

(Outer as any).prototype.format = function (this: any, t: string): string {
  // Many local `var` bindings — the HIR emits a `PreallocateBoxes`
  // statement listing each one. Pre-fix, none of these ids landed in
  // the module-wide boxed set.
  const i: string = "+02:00";
  const a: number = 5;
  const s: number = 10;
  const r: string = t || "YYYY-MM";

  return r.replace(/Y{1,4}|M{1,4}/g, function (match: string): string {
    // The capture of `i` must see the string value, not the box
    // pointer bits reinterpreted as a tiny denormal `number`.
    if (typeof i !== "string") {
      throw new Error("captured i typeof: " + typeof i);
    }
    if (match === "YYYY") return "2024";
    if (match === "MM") return pad(a + 1, 2, "0");
    return i.replace(":", "");
  } as any);
};

const o: any = new (Outer as any)();
const out = o.format("YYYY-MM");
console.log("format YYYY-MM:", out); // expect "2024-06"
// No crash means the captured `i` was seen as a string inside the
// replace callback. The exact `2024-06` byte match is the secondary
// signal; the primary regression is the `TypeError: (number).replace
// is not a function` throw — if execution reaches this line, that
// throw didn't happen.
console.log("no throw");
