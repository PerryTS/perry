// #5904 — Intl builtin functions that aren't constructors must not have an own
// `prototype` property, and must not be constructable (ECMA-262 §17: built-in
// functions only get the auto-created `.prototype` when they implement
// [[Construct]]). test262 intl402/NumberFormat/prototype/{format,formatRange,
// formatRangeToParts,resolvedOptions}/builtin.js and supportedLocalesOf/
// builtin.js all assert this on the NumberFormat method/getter functions.
//
// Perry created these via `js_closure_alloc` without flagging them
// non-constructable, so `fn.hasOwnProperty("prototype")` lazily synthesized an
// object (unlike `Math.max` / `Array.prototype.map`, which correctly report
// false). Must match `node --experimental-strip-types` byte-for-byte.
function isConstructor(f: unknown): boolean {
  try {
    Reflect.construct(function () {}, [], f as new () => unknown);
    return true;
  } catch {
    return false;
  }
}

// Assert the ECMA-262 §17 built-in-function surface for a non-constructor `fn`.
function report(label: string, fn: unknown): void {
  const f = fn as { hasOwnProperty(k: string): boolean };
  console.log(label, "tag", Object.prototype.toString.call(fn));
  console.log(label, "extensible", Object.isExtensible(fn as object));
  console.log(label, "proto===Function.prototype", Object.getPrototypeOf(fn) === Function.prototype);
  console.log(label, "hasOwn prototype", f.hasOwnProperty("prototype"));
  console.log(label, "typeof .prototype", typeof (fn as { prototype?: unknown }).prototype);
  console.log(label, "isConstructor", isConstructor(fn));
}

const nf = new Intl.NumberFormat("en-US");

// The `format` accessor (getter) and the bound [[BoundFormat]] function.
report("format getter", Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype, "format")!.get);
report("bound format", nf.format);

// Plain prototype methods.
report("resolvedOptions", Intl.NumberFormat.prototype.resolvedOptions);
report("formatToParts", Intl.NumberFormat.prototype.formatToParts);
report("formatRange", Intl.NumberFormat.prototype.formatRange);
report("formatRangeToParts", Intl.NumberFormat.prototype.formatRangeToParts);

// Static method.
report("supportedLocalesOf", Intl.NumberFormat.supportedLocalesOf);

// The fix is shared across Intl services — spot-check DateTimeFormat + Collator.
report("DTF resolvedOptions", Intl.DateTimeFormat.prototype.resolvedOptions);
report("Collator supportedLocalesOf", Intl.Collator.supportedLocalesOf);

// The constructor itself keeps [[Construct]] and its own `prototype`.
console.log("ctor isConstructor", isConstructor(Intl.NumberFormat));
console.log("ctor hasOwn prototype", Intl.NumberFormat.hasOwnProperty("prototype"));

// Regression guard: the functions still work as callables.
console.log("format works", nf.format(1234.5));
console.log("resolvedOptions.locale", nf.resolvedOptions().locale);
