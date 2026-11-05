// #3527: `eval` referenced as a VALUE (not called) must not throw a
// ReferenceError at lowering. Libraries build intrinsic tables that read it
// bare — get-intrinsic's `'%eval%': eval`. (`eval(...)` calls remain gated by
// the eval-surface classifier; this only covers the value read.)
const intrinsics: Record<string, unknown> = {
  "%Array%": Array,
  "%eval%": eval,
  "%Math%": Math,
};
console.log("has eval key:", "%eval%" in intrinsics);
console.log("Array is fn:", typeof intrinsics["%Array%"] === "function");
const e = eval;
console.log("got eval value without throwing:", e !== Symbol.for("unreachable"));
