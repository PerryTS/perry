// A module that imports its OWN namespace. Valid ESM: the namespace object
// exists before evaluation with live bindings, so the module's own body can
// read it. trails' activesupport relies on this to hand a module its own
// export set to a mixin:
//
//   import * as DateAndTimeCalculations from "./calculations.js";
//   include(RubyTime, DateAndTimeCalculations);
//
// Before this fix the whole-namespace value was `undefined` during the
// module's own init -- `@__perry_ns_<prefix>` was emitted only for modules
// some OTHER module namespace-imported, and its populator ran at the tail of
// `__init`, after the body that reads it. Per-member reads already worked,
// which is why this stayed silent: no error, no warning, exit code 0.
import * as self from "./test_gap_self_namespace_import.ts";

export function f(): number {
  return 1;
}
export const C = 7;
export class K {}

// Per-member reads: resolve to this module's own symbols.
console.log("member call      =", self.f());
console.log("member const     =", self.C);
console.log("typeof member fn =", typeof self.f);
console.log("typeof member cls=", typeof self.K);

// Whole-namespace reads DURING this module's own init -- the regression.
function describe(ns: unknown): string {
  if (ns === undefined) return "UNDEFINED";
  if (ns === null) return "NULL";
  return typeof ns + " " + JSON.stringify(Object.getOwnPropertyNames(ns as object).sort());
}
console.log("whole namespace  =", describe(self));
console.log("typeof namespace =", typeof self);

// The activesupport shape: the namespace passed to a function that enumerates
// it, at top level, exactly as `include()` does.
function mixin(target: Record<string, unknown>, mod: object): string {
  for (const key of Object.getOwnPropertyNames(mod)) {
    target[key] = (mod as Record<string, unknown>)[key];
  }
  return JSON.stringify(Object.keys(target).sort());
}
console.log("mixin keys       =", mixin({}, self));
